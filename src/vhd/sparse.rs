mod header;
use std::{cell::RefCell, collections::HashMap};
use std::path::Path;

pub use header::*;

use crate::{util::StructBuffer, math, sizes, Result, VhdFile, ReadAt, WriteAt, Flush, ImageExtent, ImageExtentOps, VhdError};

use super::{VhdImage, VhdImageExtent, VhdFooter, DEFAULT_HEADER_OFFSET, DEFAULT_TABLE_OFFSET, VhdType};

mod bat;

pub struct SparseExtent {
    file: VhdFile,
    file_path: String,
    header: VhdHeader,
    bat: RefCell<bat::VhdBat>,      
    cached_block_index: RefCell<usize>,
    cached_bitmap: RefCell<Vec<u8>>,
    cached_bitmap_dirty: RefCell<bool>,
    next_block_pos: RefCell<u64>,
    parent: Option<VhdImage>,
}

impl ReadAt for SparseExtent {
    fn read_at(&self, mut offset: u64, mut buffer: &mut [u8]) -> Result<usize> {
        let mut readed = 0_usize;
        while !buffer.is_empty() {
            match self.read_block(offset, buffer)? {
                0 => break,
                n => {
                    buffer = &mut buffer[n..];
                    offset += n as u64;
                    readed += n;
                }
            }
        }

        Ok(readed)
    }
}

impl WriteAt for SparseExtent {
    fn write_at(&self, mut offset: u64, mut data: &[u8]) -> Result<usize> {
        let mut written = 0_usize;
        while !data.is_empty() {
            match self.write_block(offset, data)? {
                0 => break,
                n => {
                    data = &data[n..];
                    offset += n as u64;
                    written += n;
                }
            }
        }

        Ok(written)
    }
}

impl Flush for SparseExtent {
    fn flush(&self) -> Result<()> {
        self.save_cached_bitmap()?;
        self.file.flush()
    }
}

impl ImageExtent for SparseExtent {
    fn backing_files(&self) -> Box<dyn core::iter::Iterator<Item = String>> {
        Box::new(std::iter::once(self.file_path.clone()))
    }

    fn storage_size(&self) -> Result<u64> {
        self.file.size()
    }
}

impl ImageExtentOps for SparseExtent {}

impl VhdImageExtent for SparseExtent {
    fn write_footer(&self, footer: &VhdFooter) -> Result<()> {
        let bytes = footer.to_bytes();
        self.file.write_all_at(0, &bytes)?;

        let next_block_pos = *self.next_block_pos.borrow();
        self.file.write_all_at(next_block_pos, &bytes)
    }

    fn sparse_header(&self) -> Option<&VhdHeader> {
        Some(&self.header)
    }

    fn file_path(&self) -> String {
        self.file_path.clone()
    }

    fn parent_locator(&self) -> Option<String> {
        let mut pl = String::from("VHD Parent Locators:\n-------------------\n");        

        for (index, loc) in self.header.prt_loc().iter().enumerate() {
            if loc.prt_loc_code() != PLAT_CODE_NONE {
                let mut buffer = vec![0_u8; loc.prt_loc_len() as usize];
                self.file.read_exact_at(loc.prt_loc_offset(), buffer.as_mut_slice()).unwrap();
                let prt_path = std::str::from_utf8(&buffer).unwrap();

                let locator = format!(
                    "{:<20}: {}
{:^20}: {}
{:^20}: {:#010X} bytes
{:^20}: {:#010X} bytes
{:^20}: {:#010X}
{:^20}: {}\n",
                    "locator", index,
                    "code", loc.prt_loc_code_str(),
                    "data_space", loc.prt_loc_space(),
                    "data_length", loc.prt_loc_len(),
                    "data_offset", loc.prt_loc_offset(),
                    "decode name", prt_path,
                );

                pl.push_str(&locator);
            }
        }

        Some(pl)
    }    
}

impl SparseExtent {
    fn new(file: VhdFile, file_path: String, header: VhdHeader, bat: bat::VhdBat, bitmap_size: u32, next_block_pos: u64) -> Self {
        SparseExtent { 
            file,
            file_path,
            header,
            bat: RefCell::new(bat),            
            cached_block_index: RefCell::new(usize::MAX),
            cached_bitmap: RefCell::new(vec![0_u8; bitmap_size as usize]),
            cached_bitmap_dirty: RefCell::new(false),
            next_block_pos: RefCell::new(next_block_pos),
            parent: None,
        }
    }

    pub(crate) fn open(file: VhdFile, file_path: String, data_offset: u64) -> Result<Self> {
        let header = VhdHeader::read(&file, data_offset)?;
        let file_size = file.size()?;

        if header.table_offset() > file_size {
            return Err(VhdError::InvalidSparseHeaderOffset);
        }

        let bat = bat::VhdBat::read(&file, header.table_offset(), header.max_bat_size())?;
        let bitmap_size = math::round_up(math::ceil(header.block_size(), sizes::SECTOR * 8), sizes::SECTOR);         
        
        let next_block_pos = file_size - sizes::SECTOR_U64;

        Ok(Self::new(file, file_path, header, bat, bitmap_size, next_block_pos))
    }

    pub(crate) fn create(file_path: String, footer: &VhdFooter, parent: Option<VhdImage>) -> Result<Self> {
        let header = VhdHeader::new(footer.current_size(), DEFAULT_TABLE_OFFSET, DD_BLOCKSIZE_DEFAULT, &parent);
        let bat = bat::VhdBat::new(header.max_bat_size());
        let bitmap_size = math::round_up(math::ceil(header.block_size(), sizes::SECTOR * 8), sizes::SECTOR);

        let file = VhdFile::create(&file_path, footer.current_size())?;
        header.write(&file, DEFAULT_HEADER_OFFSET)?;
        let bat_size = bat.write(&file, DEFAULT_TABLE_OFFSET)?;
        let mut next_block_pos = DEFAULT_TABLE_OFFSET + bat_size as u64;
        if parent.is_some() {
            let locator_size = header.write_locator(&file, next_block_pos, &parent)?;
            next_block_pos += locator_size as u64;
        } 

        let this = Self::new(file, file_path, header, bat, bitmap_size, next_block_pos);
        this.write_footer(footer)?;

        Ok(this)
    }
}

impl SparseExtent {
    fn read_block(&self, offset: u64, buffer: &mut [u8]) -> Result<usize> {
        todo!("read_block");
    }

    fn write_block(&self, offset: u64, data: &[u8]) -> Result<usize> {
        todo!("write_block");
    }

    fn save_cached_bitmap(&self) -> Result<()> {
        //todo!("save_cached_bitmap");
        Ok(())
    }
}