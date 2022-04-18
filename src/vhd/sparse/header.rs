use crate::{Uuid, UuidEx, sizes, geometry, StructBuffer, ReadAt, WriteAt, Result, AsByteSliceMut, VhdError, math};
use crate::vhd::{VhdType, vhd_time, VhdImage, DEFAULT_TABLE_OFFSET};
use std::collections::HashMap;

#[repr(C, packed)]
#[derive(Debug, Copy, Clone)]
pub struct VhdParentLocator {
    // Platform code -- see defines below
    code: u32,
    // Number of 512-byte sectors to store locator
    data_space: u32,
    // Actual length of parent locator in bytes
    data_len: u32,
    // Must be zero
    res: u32,
    // Absolute offset of locator data (bytes)
    data_offset: u64,
}

pub const PLAT_CODE_NONE: u32 = 0x0000_0000;
/// Windows relative path (UTF-16) litter endian (W2ru)
pub const PLAT_CODE_W2RU: u32 = 0x5732_7275; 
/// Windows absolute path (UTF-16) litter endian (W2ku)
pub const PLAT_CODE_W2KU: u32 = 0x5732_6B75;


#[repr(C, packed)]
#[derive(Debug, Copy, Clone)]
pub struct VhdHeader {
    // Should contain "cxsparse"
    cookie: u64,
    // Byte offset of next record. 
    data_offset: u64,
    // Absolute offset to the BAT
    table_offset: u64,
    // Version of the dd_hdr (major,minor)
    hdr_ver: u32,
    // Maximum number of entries in the BAT
    max_bat_size: u32,
    // Block size in bytes. Must be power of 2
    block_size: u32,
    // Header checksum.  1's comp of all fields
    checksum: u32,
    // ID of the parent disk
    prt_uuid: uuid::Uuid,
    // Modification time of the parent disk
    prt_ts: u32,
    // Reserved
    res1: u32,
    // Parent unicode name
    prt_name: [u16; 256],
    // Parent locator entries
    prt_loc: [VhdParentLocator; 8],
    // Reserved
    res2: [u8; 256],
}

/// (Unused) 0xffs
const DD_OFFSET: u64 = 0xFFFF_FFFF_FFFF_FFFF;
/// VHD cookie string
const DD_COOKIE: u64 = 0x6573_7261_7073_7863; /* cxsparse  big endian*/
/// Version field in VhdHeader
const DD_VERSION: u32 = 0x0001_0000;
/// Default blocksize is 2 meg
pub const DD_BLOCKSIZE_DEFAULT: u32 = 0x0020_0000; 

impl VhdHeader {
    fn swap_bytes(&mut self) {
        self.data_offset = self.data_offset.swap_bytes();
        self.table_offset = self.table_offset.swap_bytes();
        self.hdr_ver = self.hdr_ver.swap_bytes();
        self.max_bat_size = self.max_bat_size.swap_bytes();
        self.block_size = self.block_size.swap_bytes();
        self.checksum = self.checksum.swap_bytes();
        self.prt_uuid = self.prt_uuid.swap_bytes();
        self.prt_ts = self.prt_ts.swap_bytes();        

        for locator in &mut self.prt_loc {
            locator.code = locator.code.swap_bytes();
            locator.data_len = locator.data_len.swap_bytes();
            locator.data_space = locator.data_space.swap_bytes();
            locator.data_offset = locator.data_offset.swap_bytes();
        }
    }

    pub fn new(capacity: u64, table_offset: u64, block_size: u32, parent: &Option<VhdImage>) -> Self {

        let mut header = StructBuffer::<VhdHeader>::zeroed();        
        header.cookie = DD_COOKIE;
        header.data_offset = DD_OFFSET;
        header.table_offset = table_offset;
        header.hdr_ver = DD_VERSION;
        header.max_bat_size = math::ceil(capacity, block_size as u64) as u32;
        header.block_size = block_size;

        if parent.is_none() {
            header.prt_uuid = Uuid::nil();
            header.prt_ts = 0;
            header.prt_name = unsafe { std::mem::zeroed() };
            header.prt_loc = unsafe { std::mem::zeroed() };
        } else {
            let parent_footer = parent.as_ref().map(|img| img.footer()).unwrap();
            header.prt_uuid = parent_footer.uuid().clone();
            header.prt_ts = parent_footer.timestamps();

            // get utf16 parent image name
            let str_parent_path = parent.as_ref().map(|img| img.file_path()).unwrap();
            let parent_path = std::path::Path::new(&str_parent_path);

            let parent_name = parent_path
                .file_name()
                .map(|name| name.to_string_lossy()).unwrap();

            let parent_utf16_name: Vec<u16> = parent_name.encode_utf16().collect();
            header.prt_name[..parent_utf16_name.len()].copy_from_slice(&parent_utf16_name);

            // get bat size
            let bat_size = math::round_up(header.max_bat_size as usize * 4, sizes::SECTOR as usize);
            header.prt_loc[0].code = PLAT_CODE_W2KU;
            /*
             write number of bytes ('size') instead of number of sectors
             into loc->data_space to be compatible with MSFT, even though
             this goes against the specs
            */
            header.prt_loc[0].data_space = sizes::SECTOR; 
            // This field stores the actual length of the parent hard disk locator in bytes
            header.prt_loc[0].data_len = (str_parent_path.encode_utf16().count() * 2) as u32;
            header.prt_loc[0].data_offset = table_offset + bat_size as u64;
        }

        let checksum = crate::vhd::calc_header_bytes_checksum(&header);
        header.checksum = checksum;        

        header.copy()
    }

    pub fn read(stream: &impl ReadAt, pos: u64) -> Result<Self> {
        let mut header = unsafe { StructBuffer::<VhdHeader>::new() };
        stream.read_exact_at(pos, unsafe { header.as_byte_slice_mut() })?;

        if DD_COOKIE != header.cookie {
            return Err(VhdError::InvalidSparseHeaderCookie);
        }

        header.swap_bytes();

        let checksum = calc_header_checksum!(header);
        if header.checksum != checksum {
            return Err(VhdError::InvalidSparseHeaderChecksum);
        }

        Ok(header.copy())
    }

    pub fn write(&self, stream: &impl WriteAt, pos: u64) -> Result<()> {
        let mut header = unsafe { StructBuffer::<VhdHeader>::with_value(self) };
        header.swap_bytes();

        stream.write_all_at(pos, header.buffer())
    }

    pub fn write_locator(&self, stream: &impl WriteAt, pos: u64, parent: &Option<VhdImage>) -> Result<usize> {
        let parent_path = parent.as_ref().map(|img| img.file_path()).unwrap();
        let parent_path: Vec<u16> = parent_path.encode_utf16().collect();
        
        let mut temp = [0_u16; 256];
        temp[..parent_path.len()].copy_from_slice(&parent_path);
        let buf = unsafe { 
            std::slice::from_raw_parts(temp.as_ptr() as *const u8, sizes::SECTOR as usize)
        };
        stream.write_all_at(pos, buf).unwrap();

        Ok(sizes::SECTOR as usize)
    }

    pub fn table_offset(&self) -> u64 {
        self.table_offset
    }

    pub fn max_bat_size(&self) -> u32 {
        self.max_bat_size
    }

    pub fn block_size(&self) -> u32 {
        self.block_size
    }

    pub fn cookie(&self) -> &str {
        let cookie = unsafe {
            std::slice::from_raw_parts(&self.cookie as *const _ as *const u8, 8)
        };

        std::str::from_utf8(cookie).unwrap()
    }

    pub fn prt_name(&self) -> String {
        String::from_utf16_lossy(&self.prt_name)
    }
    
    pub fn prt_loc(&self) -> &[VhdParentLocator] {
        &self.prt_loc
    }
}

impl VhdParentLocator {
    pub fn prt_loc_code(&self) -> u32 {
        self.code    
    }

    pub fn prt_loc_code_str(&self) -> String {
        let loc_code = self.code.swap_bytes();
        let loc_code = unsafe {
            std::slice::from_raw_parts(&loc_code as *const _ as *const u8, 4)
        };

        String::from(std::str::from_utf8(loc_code).unwrap())
    }

    pub fn prt_loc_space(&self) -> u32 {
        self.data_space    
    }

    pub fn prt_loc_len(&self) -> u32 {
        self.data_len
    }

    pub fn prt_loc_offset(&self) -> u64 {
        self.data_offset
    }
}

impl std::fmt::Display for VhdHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("VHD Header Summary:\n-------------------\n")?;        


        let header = format!(
            "{:<20}: {}
{:<20}: {:#018X}
{:<20}: {:#018X}
{:<20}: Major: {}, Minor: {}
{:<20}: {}
{:<20}: {} Mb, ({} bytes)
{:<20}: {}
{:<20}: {}
{:<20}: {:#010X}
{:<20}: {:#010X}\n",
            "Cookie",  self.cookie(),
            "Data offset (unused)", self.data_offset,
            "Table offset",  self.table_offset,
            "Header version", self.hdr_ver >> 16, self.hdr_ver >> 24,
            "Max BAT size", self.max_bat_size,
            "Block size", self.block_size >> 20, self.block_size,
            "Parent name", self.prt_name(),
            "Parent UUID", self.prt_uuid.to_string(),
            "Parent timestamp", self.prt_ts,
            "Checksum", self.checksum,            
        );        

        f.write_str(&header)        
    }
}
