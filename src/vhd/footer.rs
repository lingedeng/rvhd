use crate::{Uuid, UuidEx, sizes, geometry, StructBuffer, ReadAt, Result, AsByteSliceMut, VhdError, AsByteSlice, Geometry};
use super::{VhdType, vhd_time, vhd_type_str};

#[repr(C, packed)]
#[derive(Debug, Copy, Clone)]
pub struct DiskGeometry {
    cylinders: u16,
    heads: u8,
    sectors_per_track: u8,
}

#[repr(C, packed)]
#[derive(Debug, Copy, Clone)]
pub struct VhdFooter {
    // Identifies original creator of the disk
    cookie: u64,
    // Feature Support
    features: u32,
    // (major,minor) version of disk file
    ff_version: u32,
    // Abs. offset from SOF to next structure
    data_offset: u64,
    // Creation time.  secs since 1/1/2000GMT
    timestamps: u32,
    // Creator application
    crtr_app: u32,
    // Creator version (major,minor)
    crtr_ver: u32,
    // Creator host OS
    crtr_os: u32,
    // Size at creation (bytes)
    orig_size: u64,
    // Current size of disk (bytes)
    curr_size: u64,
    // Disk geometry
    geometry: DiskGeometry,
    // Disk type
    disk_type: u32,
    // 1's comp sum of this struct
    checksum: u32,
    // Unique disk ID, used for naming parents
    uuid: uuid::Uuid,
    // one-bit -- is this disk/VM in a saved state
    saved: u8,
    // padding
    reserved: [u8; 427],
}

/// VHD cookie string
const HD_COOKIE: u64 = 0x7869_7463_656E_6F63; // big endian "conectix"

/// Feature fields in VhdFooter
const HD_NO_FEATURES: u32 = 0x0000_0000;
const HD_TEMPORARY: u32 = 0x0000_0001;
const HD_RESERVED: u32 = 0x0000_0002;

/// Version field in VhdFooter
const HD_FF_VERSION: u32 = 0x0001_0000;

const HD_CR_OS_WINDOWS: u32 = 0x6B32_6957; /* (Wi2k) big endian */
const HD_CR_OS_MAC: u32 = 0x2063_614D; /* (Mac ) big endian */

const HD_CR_APP: u32 = 0x6468_7672; /* rvhd big endian*/
const HD_CR_VERSION: u32 = 0x0001_0000;

impl VhdFooter {
    pub fn swap_bytes(&mut self) {
        self.features = self.features.swap_bytes();
        self.ff_version = self.ff_version.swap_bytes();
        self.data_offset = self.data_offset.swap_bytes();
        self.timestamps = self.timestamps.swap_bytes();
        self.crtr_ver = self.crtr_ver.swap_bytes();
        self.orig_size = self.orig_size.swap_bytes();
        self.curr_size = self.curr_size.swap_bytes();
        self.geometry.cylinders = self.geometry.cylinders.swap_bytes();
        self.disk_type = self.disk_type.swap_bytes();
        self.checksum = self.checksum.swap_bytes();

        self.uuid = self.uuid.swap_bytes();
    }

    pub fn new(size: u64, disk_type: VhdType) -> Self {
        let data_offset = match disk_type {
            VhdType::Fixed => 0xFFFF_FFFF_FFFF_FFFF,
            _ => sizes::SECTOR_U64,
        };

        let vhd_time = vhd_time();
        let vhd_uuid = Uuid::new_v4();

        let geo = geometry::Geometry::with_vhd_capacity(size);
        let vhd_geo = DiskGeometry {
            cylinders: geo.cylinders as u16,
            heads: geo.heads as u8,
            sectors_per_track: geo.sectors_per_track as u8,
        };

        use num_traits::ToPrimitive;
        let disk_type = disk_type.to_u32().unwrap();
        
        let mut footer = StructBuffer::<VhdFooter>::zeroed();        
        footer.cookie = HD_COOKIE;
        footer.features = HD_RESERVED;
        footer.ff_version = HD_FF_VERSION;
        footer.data_offset = data_offset;
        footer.timestamps = vhd_time;
        footer.crtr_app = HD_CR_APP;
        footer.crtr_ver = HD_CR_VERSION;
        footer.crtr_os = HD_CR_OS_WINDOWS;
        footer.orig_size = size;
        footer.curr_size = size;
        footer.geometry = vhd_geo;
        footer.disk_type = disk_type;
        footer.uuid = vhd_uuid;

        let checksum = super::calc_header_bytes_checksum(&footer);
        footer.checksum = checksum;        

        footer.copy()
    }

    pub fn read(stream: &impl ReadAt, pos: u64) -> Result<Self> {
        let mut footer = unsafe { StructBuffer::<VhdFooter>::new() };
        stream.read_exact_at(pos, unsafe {
           footer.as_byte_slice_mut() 
        })?;

        if HD_COOKIE != footer.cookie {
            return Err(VhdError::InvalidHeaderCookie);
        }

        footer.swap_bytes();

        let checksum = calc_header_checksum!(footer);
        if footer.checksum != checksum {
            return Err(VhdError::InvalidHeaderChecksum);
        }
        
        let disk_type: VhdType = match num_traits::FromPrimitive::from_u32(footer.disk_type) {
            Some(kind) => kind,
            _ => return Err(VhdError::UnknownVhdType(footer.disk_type)),
        };

        Ok(footer.copy())
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut footer = unsafe { StructBuffer::<VhdFooter>::with_value(self) };
        footer.swap_bytes();

        let slice = unsafe { footer.as_byte_slice() };
        slice.to_vec()
    }

    pub fn geometry(&self) -> Geometry {
        Geometry { 
            cylinders: self.geometry.cylinders as u64,
            heads: self.geometry.heads as u32,
            sectors_per_track: self.geometry.sectors_per_track as u32,
            bytes_per_sector: sizes::SECTOR,
        }
    }

    pub fn uuid(&self) -> &Uuid {
        &self.uuid
    }

    pub fn current_size(&self) -> u64 {
        self.curr_size
    }

    pub fn disk_type(&self) -> VhdType {
        num_traits::FromPrimitive::from_u32(self.disk_type).unwrap()
    }

    pub fn data_offset(&self) -> u64 {
        self.data_offset
    }

    pub fn timestamps(&self) -> u32 {
        self.timestamps
    } 
    
    pub fn cookie(&self) -> &str {        
        let cookie = unsafe {
            std::slice::from_raw_parts(&self.cookie as *const _ as *const u8, 8)
        };

        std::str::from_utf8(cookie).unwrap()
    }

    pub fn crtr_app(&self) -> &str {
        //let crtr_app = self.crtr_app;
        let crtr_app = unsafe {
            std::slice::from_raw_parts(&self.crtr_app as *const _ as *const u8, 4)
        };

        std::str::from_utf8(crtr_app).unwrap()
    }

    pub fn crtr_os(&self) -> &str {
        //let crtr_os = self.crtr_os;
        let crtr_os = unsafe {
            std::slice::from_raw_parts(&self.crtr_os as *const _ as *const u8, 4)
        };

        std::str::from_utf8(crtr_os).unwrap()
    }
}

impl std::fmt::Display for VhdFooter {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("VHD Footer Summary:\n-------------------\n")?;        


        let footer = format!(
            "{:<20}: {}
{:<20}: {:#010X}
{:<20}: Major: {}, Minor: {}
{:<20}: {:#018X}
{:<20}: {:#08X}
{:<20}: {}
{:<20}: Major: {}, Minor: {}
{:<20}: {}
{:<20}: {} Mb, ({} bytes)
{:<20}: {} Mb, ({} bytes)
{:<20}: Cyl: {}, Hds: {}, Sctrs: {}
{:<20}: {}
{:<20}: {:#08X}
{:<20}: {}\n",
            "Cookie",  self.cookie(),
            "Features", self.features,
            "File format version",  self.ff_version >> 16, self.ff_version >> 24,
            "Data offset", self.data_offset,
            "Timestamp", self.timestamps,
            "Creator Application", self.crtr_app(),
            "Creator version", self.crtr_ver >> 16, self.crtr_ver >> 24,
            "Creator OS", self.crtr_os(),
            "Original disk size", self.orig_size >> 20, self.orig_size,
            "Current disk size", self.curr_size >> 20, self.curr_size,
            "Geometry", self.geometry.cylinders, self.geometry.heads, self.geometry.sectors_per_track,
            "Disk type", vhd_type_str(self.disk_type()),
            "Checksum", self.checksum,
            "UUID", self.uuid().to_string(),
        );

        f.write_str(&footer)
    }
}