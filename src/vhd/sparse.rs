mod header;
use std::cell::{RefCell, Ref};
use std::path::{Path, MAIN_SEPARATOR};

pub use header::*;

use crate::{AsByteSliceMut, StructBuffer, AsByteSlice};
use crate::{util, math, sizes, Result, VhdFile, ReadAt, WriteAt, Flush, SeekAt, ImageExtent, ImageExtentOps, VhdError};

use super::{VhdImage, VhdImageExtent, VhdFooter, DEFAULT_HEADER_OFFSET, DEFAULT_TABLE_OFFSET, VhdType};

pub mod bat;

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

impl SeekAt for SparseExtent {
    fn seek_at(&self, pos: std::io::SeekFrom) -> Result<u64> {
        self.file.seek_at(pos)
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
    
    fn parent_locator_data(&self, index: usize) -> Option<Vec<u8>> {
        let locator = self.header.prt_loc()[index];

        let mut buffer = vec![0_u8; locator.prt_loc_space() as usize];
        self.file.read_exact_at(locator.prt_loc_offset(), buffer.as_mut_slice()).unwrap();

        Some(buffer)
    }

    fn sparse_bat(&self) -> Option<&RefCell<bat::VhdBat>> {
        Some(&self.bat)
    }

    fn sparse_block_bitmap(&self, bat_block_index: usize) -> Option<(u64, &RefCell<Vec<u8>>)> {
        let bitmap_offset = self.calc_bitmap_pos(bat_block_index).unwrap();
        self.populate_block_bitmap(bat_block_index).unwrap();

        Some((bitmap_offset, &self.cached_bitmap))
    }

    fn sparse_block_data(&self, bat_block_index: usize, buffer: &mut [u8]) -> Result<u64> {
        let block_offset = self.calc_sector_pos(bat_block_index, 0)?;
        let _ = self.read_block(block_offset, buffer)?;

        Ok(block_offset)
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
        let (header, relative_utf16_path) = VhdHeader::new(footer.current_size(), DEFAULT_TABLE_OFFSET, DD_BLOCKSIZE_DEFAULT, &file_path, &parent);
        let bat = bat::VhdBat::new(header.max_bat_size());
        let bitmap_size = math::round_up(math::ceil(header.block_size(), sizes::SECTOR * 8), sizes::SECTOR);        
        
        let file = VhdFile::create(&file_path, footer.current_size())?;
        header.write(&file, DEFAULT_HEADER_OFFSET)?;
        let bat_size = bat.write(&file, DEFAULT_TABLE_OFFSET)?;
        let mut next_block_pos = DEFAULT_TABLE_OFFSET + bat_size as u64;
        if parent.is_some() {
            for i in 0..2 as usize {
                // write W2ku and W2ru
                let locator_size = header.write_locator(&file, i, &relative_utf16_path)?;
                next_block_pos += locator_size as u64;
            }            
        } 

        let this = Self::new(file, file_path, header, bat, bitmap_size, next_block_pos);
        this.write_footer(footer)?;

        Ok(this)
    }    
}

const INVALID_CACHE_INDEX: usize = usize::max_value();

fn calc_sector_mask(sector_in_block: usize) -> u8 {
    1 << (7 - (sector_in_block % 8) as u8)
}

impl SparseExtent {
    fn populate_block_bitmap(&self, index: usize) -> Result<bool> {
        if *self.cached_block_index.borrow() == index {
            return Ok(true);
        }

        let block_id = self.bat.borrow().block_id(index)?;
        if block_id == bat::DD_BLOCK_UNUSED {
            return Ok(true);
        }

        self.save_cached_bitmap()?;

        let bitmap_pos = block_id as u64 * sizes::SECTOR_U64;
        self.file
            .read_exact_at(bitmap_pos, self.cached_bitmap.borrow_mut().as_mut_slice())?;
        *self.cached_block_index.borrow_mut() = index;

        Ok(true)
    }

    fn save_cached_bitmap(&self) -> Result<()> {
        let cached_block_index = *self.cached_block_index.borrow();
        let mut cached_bitmap_dirty = self.cached_bitmap_dirty.borrow_mut();
        if cached_block_index == INVALID_CACHE_INDEX || !(*cached_bitmap_dirty) {
            return Ok(());
        }

        let cached_block_id = self.bat.borrow().block_id(cached_block_index)?;
        if cached_block_id == bat::DD_BLOCK_UNUSED {
            return Err(VhdError::UnexpectedBlockId(cached_block_index, cached_block_id));
        }

        let bitmap_pos = cached_block_index as u64 * sizes::SECTOR_U64;
        self.file
            .write_all_at(bitmap_pos, self.cached_bitmap.borrow_mut().as_mut_slice())?;
        *cached_bitmap_dirty = false;

        Ok(())
    }

    fn check_sector_mask(&self, index: usize, sector_in_block: u32) -> Result<bool> {
        if *self.cached_block_index.borrow() != index {
            let res = self.populate_block_bitmap(index)?;
            if !res {
                return Ok(false);
            }
        }

        debug_assert_eq!(*self.cached_block_index.borrow(), index);

        let sector_in_block = sector_in_block as usize;
        let sector_mask = calc_sector_mask(sector_in_block);
        let is_bit_set = self.cached_bitmap.borrow()[sector_in_block / 8] & sector_mask != 0;

        Ok(is_bit_set)
    }

    fn read_sectors(&self, to_read: u32, block_index: usize, sector_in_block: u32) -> Result<(bool, usize)> {
        let to_read_in_sectors = to_read / sizes::SECTOR;
        // remember first sector bit (valid data\parent or not)
        let first_sector_bit = self.check_sector_mask(block_index, sector_in_block)?;

        // now look for subsequent sectors bits and stop if sector bit is different (or no more sectors)
        let mut sectors_count = 1_u32;
        while sectors_count < to_read_in_sectors {
            let sector_bit = self.check_sector_mask(block_index, sector_in_block + sectors_count)?;
            if sector_bit != first_sector_bit {
                break;
            }

            sectors_count += 1;
        }

        Ok((first_sector_bit, (sectors_count * sizes::SECTOR) as usize))
    }

    fn calc_sector_pos(&self, block_index: usize, sector_in_block: u32) -> Result<u64> {
        let block_id = self.bat.borrow().block_id(block_index)?;

        if block_id == bat::DD_BLOCK_UNUSED {
            return Err(VhdError::UnexpectedBlockId(block_index, block_id));
        }

        Ok(((block_id + sector_in_block) as u64) * sizes::SECTOR_U64 + self.cached_bitmap.borrow().len() as u64)
    }

    fn calc_bitmap_pos(&self, block_index: usize) -> Result<u64> {
        let block_id = self.bat.borrow().block_id(block_index)?;

        if block_id == bat::DD_BLOCK_UNUSED {
            return Err(VhdError::UnexpectedBlockId(block_index, block_id));
        }

        Ok(block_id as u64 * sizes::SECTOR_U64)
    }

    fn read_parent_or_zero(&self, offset: u64, buffer: &mut [u8]) -> Result<usize> {
        match &self.parent {
            Some(p) => p.read_at(offset, buffer),
            None => {
                for b in buffer.iter_mut() {
                    *b = 0;
                }

                Ok(buffer.len())
            }
        }
    }

    fn read_block_data(&self, block_index: usize, offset_in_block: u32, buffer: &mut [u8]) -> Result<(bool, usize)> {
        let sector_in_block = offset_in_block / sizes::SECTOR;
        let offset_in_sector = offset_in_block % sizes::SECTOR;
        let to_read = buffer.len() as u32;

        let (data_exist, data_buffer) = if offset_in_sector != 0 || to_read < sizes::SECTOR {
            // read at non sector boundary
            let data_exist = self.check_sector_mask(block_index, sector_in_block)?;
            (data_exist, buffer)
        } else {
            // read as many full sectors as possible
            let (data_exist, valid_len) = self.read_sectors(to_read, block_index, sector_in_block)?;
            (data_exist, &mut buffer[..valid_len])
        };

        if data_exist {
            let sector_pos = self.calc_sector_pos(block_index, sector_in_block)?;
            let data_offset = sector_pos + offset_in_sector as u64;

            self.file.read_at(data_offset, data_buffer).map(|sz| (true, sz))
        } else {
            let offset = block_index as u64 * self.header.block_size() as u64 + offset_in_block as u64;
            self.read_parent_or_zero(offset, data_buffer).map(|sz| (false, sz))
        }
    }

    fn read_block(&self, offset: u64, buffer: &mut [u8]) -> Result<usize> {
        let block_size = self.header.block_size() as u64;
        let block_index = (offset / block_size) as usize;
        let offset_in_block = (offset % block_size) as u32;
        let to_read = std::cmp::min(buffer.len() as u32, self.header.block_size() - offset_in_block);
        let block_buffer = &mut buffer[..to_read as usize];

        let block_in_current_file = self.populate_block_bitmap(block_index)?;
        if block_in_current_file {
            self.read_block_data(block_index, offset_in_block, block_buffer).map(|r| r.1)
        } else {
            self.read_parent_or_zero(offset, block_buffer)
        }
    }

    fn write_block(&self, offset: u64, data: &[u8]) -> Result<usize> {
        let block_size = self.header.block_size() as u64;
        let block_index = (offset / block_size) as usize;

        let block_in_current_file = self.populate_block_bitmap(block_index)?;
        if !block_in_current_file {
            self.allocate_block(block_index)?;
        }

        let offset_in_block = (offset % block_size) as u32;
        let sector_in_block = offset_in_block / sizes::SECTOR;
        let offset_in_sector = offset_in_block % sizes::SECTOR;
        let mut to_write = std::cmp::min(data.len(), (self.header.block_size() - offset_in_block) as usize);

        if offset_in_sector != 0 || to_write < (sizes::SECTOR as usize) {
            // reduce size to the end of the sector
            to_write = std::cmp::min(data.len(), (sizes::SECTOR - offset_in_sector) as usize);

            // read the sector
            let mut sector_buffer = unsafe { util::alloc_buffer(sizes::SECTOR as usize) };
            let sector_offset_in_block = math::round_down(offset_in_sector, sizes::SECTOR);
            let (data_exist, _) = self.read_block_data(block_index, sector_offset_in_block, &mut sector_buffer)?;

            // update it
            let start = offset_in_sector as usize;
            let end = start + to_write;
            sector_buffer[start..end].copy_from_slice(&data[..to_write]);

            // and write back
            let pos = self.calc_sector_pos(block_index, sector_in_block)?;
            self.file.write_all_at(pos, &sector_buffer)?;

            if !data_exist {
                // the sector was read from the parent
                self.mark_cached_bitmap_dirty(sector_in_block as usize);
            }
        } else {
            // write as much whole sectors as possible
            to_write = math::round_down(to_write, sizes::SECTOR as usize);
            let pos = self.calc_sector_pos(block_index, sector_in_block)?;
            self.file.write_all_at(pos, &data[..to_write])?;

            // update bitmap bits for written sectors
            let mut i = 0;
            let mut sector_in_block = sector_in_block as usize;
            while i < to_write {
                self.mark_cached_bitmap_dirty(sector_in_block);
                sector_in_block += 1;
                i += sizes::SECTOR as usize;
            }
        }

        Ok(to_write)
    }
    
    fn allocate_block(&self, block_index: usize) -> Result<()> {
        let block_id = self.bat.borrow().block_id(block_index)?;
        if block_id != bat::DD_BLOCK_UNUSED {
            return Err(VhdError::UnexpectedBlockId(block_index, block_id));
        }

        self.save_cached_bitmap()?;

        let mut bitmap = self.cached_bitmap.borrow_mut();
        // initial block bitmap should be zeroed
        unsafe { std::ptr::write_bytes(bitmap.as_mut_ptr(), 0, bitmap.len()) }

        let mut next_block_pos = self.next_block_pos.borrow_mut();
        let block_pos = *next_block_pos;
        *next_block_pos += bitmap.len() as u64 + self.header.block_size() as u64;
        *self.cached_block_index.borrow_mut() = block_index;

        if block_pos < self.file.size()? {
            // The footer is here! Have to override it with zeroes.
            let zeroes = StructBuffer::<VhdFooter>::zeroed();
            self.file.write_all_at(block_pos, zeroes.buffer())?;
        }

        // write one byte at the end of the block to expand the file (OS will fill it with zeroes)
        self.file.write_all_at(*next_block_pos - 1, unsafe { 0_u8.as_byte_slice() })?;

        // update BAT in memory...
        let block_pos_in_sectors = (block_pos / sizes::SECTOR_U64) as u32;
        self.bat.borrow_mut().set_block_id(block_index, block_pos_in_sectors)?;

        // ...and in the file
        let swapped_id = block_pos_in_sectors.swap_bytes();
        let raw_block_pos_in_sectors_pos = self.header.table_offset() + (block_index as u64 * 4);
        self.file
            .write_all_at(raw_block_pos_in_sectors_pos, unsafe { swapped_id.as_byte_slice() })?;
        
        // TODO: It might be usefull to write VHD footer after each block allocation.
        //       This will reduce speed but greatly increase error tolerance.

        Ok(())
    }
    
    fn mark_cached_bitmap_dirty(&self, sector_in_block: usize) {
        let sector_mask = calc_sector_mask(sector_in_block);
        self.cached_bitmap.borrow_mut()[sector_in_block / 8] |= sector_mask;
        *self.cached_bitmap_dirty.borrow_mut() = true;
    }
}