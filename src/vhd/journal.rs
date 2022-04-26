use crate::vhd::calc_header_bytes_checksum;
use crate::{Uuid, sizes, StructBuffer, ReadAt, Result, AsByteSliceMut, VhdError, AsByteSlice, VhdFile, WriteAt, SeekAt, Flush, math};
use super::{VhdType, VhdImage, VhdFooter, VhdHeader};
use std::cell::RefCell;
use std::mem;

// whether record block bitmap or/and data
pub const VHD_JOURNAL_METADATA: u32 = 0x01;
pub const VHD_JOURNAL_DATA:u32 = 0x02;

#[derive(Debug, Copy, Clone, FromPrimitive, ToPrimitive, Eq, PartialEq)]
//#[warn(non_camel_case_types)]
enum VhdJournalEntryType {
    VhdJournalEntryTypeFooterP = 0x01,
    VhdJournalEntryTypeFooterC = 0x02,
    VhdJournalEntryTypeHeader = 0x03,
    VhdJournalEntryTypeLocator = 0x04,
    VhdJournalEntryTypeBat = 0x05,
    VhdJournalEntryTypeData = 0x06,
}

const VHD_JOURNAL_HEADER_COOKIE:u64 = 0x6c61_6e72_756f_6a76; /* vjournal (big endian) */
const VHD_JOURNAL_ENTRY_COOKIE:u64 = 0xaaaa_1234_4321_aaaa;

#[repr(C, packed)]
#[derive(Debug, Copy, Clone)]
struct VhdJournalHeader {
    cookie: u64,
    uuid: uuid::Uuid,
    vhd_footer_offset: u64,
    journal_data_entries: u32,
    journal_metadata_entries: u32,
    journal_data_offset: u64,
    journal_metadata_offset: u64,
    journal_eof: u64,
    pad: [u8; 448],
}

struct VhdJournal {
    jfile: VhdFile,
    jfile_path: String,
    vhd_journal_header: RefCell<VhdJournalHeader>,
    vhd_image: VhdImage,
}

#[repr(C, packed)]
#[derive(Debug, Copy, Clone)]
struct VhdJournalEntry {
    cookie: u64,
    etype: u32,
    size: u32,
    offset: u64,
    checksum: u32,
    reserved: u32,
}

impl VhdJournalHeader {
    fn new() -> Self {
        VhdJournalHeader {
            cookie: VHD_JOURNAL_HEADER_COOKIE,
            uuid: Uuid::nil(),
            vhd_footer_offset: 0_u64,
            journal_data_entries: 0_u32,
            journal_metadata_entries: 0_u32,
            journal_data_offset: 0_u64,
            journal_metadata_offset: 0_u64,
            journal_eof: 0_u64,
            pad: [0_u8; 448],
        }
    }

    fn swap_bytes(&mut self) {
        // FIXME: uuid swap_bytes cause write failed with fixed vhd        
        //self.uuid = self.uuid.swap_bytes();        

        self.vhd_footer_offset = self.vhd_footer_offset.swap_bytes();
        self.journal_data_entries = self.journal_data_entries.swap_bytes();
        self.journal_metadata_entries = self.journal_metadata_entries.swap_bytes();
        self.journal_data_offset = self.journal_data_offset.swap_bytes();
        self.journal_metadata_offset = self.journal_metadata_offset.swap_bytes();
        self.journal_eof = self.journal_eof.swap_bytes();        
    }
}

impl VhdJournalEntry {
    fn new(etype: VhdJournalEntryType, size: u32, offset: u64) -> Self {
        use num_traits::ToPrimitive;
        let etype = etype.to_u32().unwrap();

        let mut entry = StructBuffer::<VhdJournalEntry>::zeroed();
        entry.cookie = VHD_JOURNAL_ENTRY_COOKIE;
        entry.etype = etype;
        entry.size = size;
        entry.offset = offset;
        entry.checksum = 0;
        entry.reserved = 0;

        let checksum = calc_header_bytes_checksum(&entry);
        entry.checksum = checksum;

        entry.copy()
    }

    fn swap_bytes(&mut self) {
        self.etype = self.etype.swap_bytes();
        self.size = self.size.swap_bytes();
        self.offset = self.offset.swap_bytes();
        self.checksum = self.checksum.swap_bytes();
    }
}

impl VhdJournal {
    pub fn create<S: Into<String>>(img: VhdImage, jpath: S) -> Result<Self> {
        let jpath = jpath.into();
        let jfile = VhdFile::create(&jpath, 0)?;
        let off = img.file_size()?;        

        let mut header = VhdJournalHeader::new();
        header.uuid = img.id().clone();        
        header.vhd_footer_offset = off - mem::size_of::<VhdFooter>() as u64;
        header.journal_eof = mem::size_of::<VhdJournalHeader>() as u64;

        println!("header.uuid: {}, footer_offset: {}, journal_eof: {}",
            header.uuid.to_string(), header.vhd_footer_offset, header.journal_eof);

        let this = VhdJournal {
            jfile,
            jfile_path: jpath,
            vhd_journal_header: RefCell::new(header),
            vhd_image: img,
        };        
        
        this.journal_write_header()?;
        this.journal_add_metadata()?;

        Ok(this)
    }

    pub fn open<S: Into<String>>(img: &VhdImage, jpath: S) -> Result<Self> {
        todo!("open");
    }

    pub fn add_block(&self, bat_block_index: usize, mode: u32) -> Result<()> {
        match self.vhd_image.disk_type() {
            VhdType::Fixed => return Err(VhdError::NeedDyncOrDiffImage),
            _ => (),
        }
        
        if (mode | VHD_JOURNAL_METADATA) == VHD_JOURNAL_METADATA {
            let pos = self.vhd_journal_header.borrow().journal_eof;

            let (offset, bitmap) = self.vhd_image.sparse_block_bitmap(bat_block_index).unwrap();
            let entry = VhdJournalEntry::new(
                VhdJournalEntryType::VhdJournalEntryTypeData, 
                bitmap.borrow().len() as u32,
                offset);
            self.journal_update(pos, entry, unsafe { bitmap.borrow().as_byte_slice() })?;
        }

        if (mode | VHD_JOURNAL_DATA) == VHD_JOURNAL_DATA {
            let pos = self.vhd_journal_header.borrow().journal_eof;
            let img_header = self.vhd_image.sparse_header().unwrap();
            let mut buffer = Vec::with_capacity(img_header.block_size() as usize);
            let offset = self.vhd_image.sparse_block_data(bat_block_index, &mut buffer)?;

            let entry = VhdJournalEntry::new(
                VhdJournalEntryType::VhdJournalEntryTypeData, 
                img_header.block_size(),
                offset);
            self.journal_update(pos, entry, &buffer)?;
        }

        Ok(())
    }

    pub fn commit(&self) -> Result<()> {
        todo!("commit");
    }

    pub fn revert(&self) -> Result<()> {
        todo!("revert");
    }    

    fn journal_write_header(&self) -> Result<()> {  
        let jheader = self.vhd_journal_header.clone().into_inner();         
        let mut sheader = unsafe { StructBuffer::<VhdJournalHeader>::with_value(&jheader) };
        sheader.swap_bytes();

        //println!("{:?}", header.copy());

        self.jfile.write_all_at(0, sheader.buffer())
    }

    fn journal_add_metadata(&self) -> Result<()> {        
        self.journal_add_footer()?;
        
        match self.vhd_image.disk_type() {
            VhdType::Fixed => return Ok(()),
            _ => (),
        }

        self.journal_add_header()?;
        self.journal_add_locators()?;
        self.journal_add_bat()?;        
        
        Ok(())
    }

    fn journal_add_footer(&self) -> Result<()> {
        let pos = self.vhd_journal_header.borrow().journal_eof;
        let offset = self.vhd_journal_header.borrow().vhd_footer_offset;

        let footer = unsafe { StructBuffer::<VhdFooter>::with_value(self.vhd_image.footer()) };        
        let entry = VhdJournalEntry::new(
            VhdJournalEntryType::VhdJournalEntryTypeFooterP, 
            mem::size_of::<VhdFooter>() as u32, 
            offset);
        self.journal_update(pos, entry, footer.buffer())?;

        match self.vhd_image.disk_type() {
            VhdType::Fixed => return Ok(()),
            _ => (),
        }

        let pos = self.vhd_journal_header.borrow().journal_eof;        

        //self.vhd_image.read_exact_at(0, unsafe { footer.as_byte_slice_mut() })?;
        let entry = VhdJournalEntry::new(
            VhdJournalEntryType::VhdJournalEntryTypeFooterC, 
            mem::size_of::<VhdFooter>() as u32, 
            0);
        self.journal_update(pos, entry, footer.buffer())?;
        
        Ok(())
    }

    fn journal_add_header(&self) -> Result<()> {
        let offset = self.vhd_image.footer().data_offset();
        let pos = self.vhd_journal_header.borrow().journal_eof;

        let header = unsafe { StructBuffer::<VhdHeader>::with_value(self.vhd_image.sparse_header().unwrap()) };
        //self.vhd_image.read_exact_at(offset, unsafe { header.as_byte_slice_mut() })?;

        let entry = VhdJournalEntry::new(
            VhdJournalEntryType::VhdJournalEntryTypeHeader, 
            mem::size_of::<VhdHeader>() as u32, 
            offset);
        self.journal_update(pos, entry, header.buffer())?;

        Ok(())
    }

    fn journal_add_locators(&self) -> Result<()> {        
        let img_header = self.vhd_image.sparse_header().unwrap();

        for (i, locator) in img_header.prt_loc().iter().enumerate() {
            if locator.prt_loc_code() != super::sparse::PLAT_CODE_NONE {
                let pos = self.vhd_journal_header.borrow().journal_eof;
                let data = self.vhd_image.parent_locator_data(i).unwrap();

                let entry = VhdJournalEntry::new(
                    VhdJournalEntryType::VhdJournalEntryTypeLocator, 
                    locator.prt_loc_space(), 
                    locator.prt_loc_offset());

                self.journal_update(pos, entry, &data)?;
            }
        }

        Ok(())
    }

    fn journal_add_bat(&self) -> Result<()> {
        let pos = self.vhd_journal_header.borrow().journal_eof;
        let img_header = self.vhd_image.sparse_header().unwrap();

        let size = math::round_up((img_header.max_bat_size() * 4) as usize, sizes::SECTOR as usize);
        let entry = VhdJournalEntry::new(
            VhdJournalEntryType::VhdJournalEntryTypeBat, 
            size as u32, 
            img_header.table_offset());

        let data = self.vhd_image.sparse_bat().unwrap().borrow();
        let data = data.bat_data();
        let data = unsafe {
            std::slice::from_raw_parts(data.as_ptr() as *const u8, data.len() * 4)
        };
        self.journal_update(pos, entry, data)?;

        Ok(())
    }

    fn journal_update(&self, pos: u64, entry: VhdJournalEntry, entry_data: &[u8]) -> Result<()> {
        let mut entry_buf = unsafe { StructBuffer::<VhdJournalEntry>::with_value(&entry) };
        entry_buf.swap_bytes();        
        
        self.jfile.write_all_at(pos, entry_buf.buffer())?;
        self.jfile.write_all_at(pos + mem::size_of::<VhdJournalEntry>() as u64, entry_data)?;

        let entry_type = num_traits::FromPrimitive::from_u32(entry.etype).unwrap();
        let data_offset = self.vhd_journal_header.borrow().journal_eof;        
        match entry_type {
            VhdJournalEntryType::VhdJournalEntryTypeData => {                
                {               
                    if self.vhd_journal_header.borrow().journal_data_entries == 0 {
                        self.vhd_journal_header.borrow_mut().journal_data_offset = data_offset;
                    }
                }
                self.vhd_journal_header.borrow_mut().journal_data_entries += 1;
            },
            _ => {                
                if self.vhd_journal_header.borrow().journal_metadata_entries == 0 {
                    self.vhd_journal_header.borrow_mut().journal_metadata_offset = data_offset;
                }
                
                self.vhd_journal_header.borrow_mut().journal_metadata_entries += 1;                
            },
        }
                
        self.vhd_journal_header.borrow_mut().journal_eof += (mem::size_of::<VhdJournalEntry>() + entry_data.len()) as u64;
        
        self.journal_write_header()?;
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_journal_new_test() {
        let img = VhdImage::open("D:\\123.vhd").unwrap();

        let journal = VhdJournal::create(img, "D:\\123_journal").unwrap();

        assert_eq!(journal.vhd_journal_header.borrow().journal_metadata_entries, 1);
        assert_eq!(journal.vhd_journal_header.borrow().journal_metadata_offset, mem::size_of::<VhdJournalHeader>() as u64);
    }

    #[test]
    fn dynamic_journal_new_test() {
        let img = VhdImage::open("D:\\456.vhd").unwrap();

        let journal = VhdJournal::create(img, "D:\\456_journal").unwrap();

        assert_eq!(journal.vhd_journal_header.borrow().journal_metadata_entries, 4);
        assert_eq!(journal.vhd_journal_header.borrow().journal_metadata_offset, mem::size_of::<VhdJournalHeader>() as u64);        
    }

    #[test]
    fn diff_journal_new_test() {
        let img = VhdImage::open("D:\\567.vhd").unwrap();

        let journal = VhdJournal::create(img, "D:\\567_journal").unwrap();

        assert_eq!(journal.vhd_journal_header.borrow().journal_metadata_entries, 6);
        assert_eq!(journal.vhd_journal_header.borrow().journal_metadata_offset, mem::size_of::<VhdJournalHeader>() as u64);
    }
}
