use super::*;
use crate::{ImageExtent, ReadAt, WriteAt, Flush, SeekAt, VhdFile, sizes};


pub struct FixedExtent {
    file: VhdFile,
    file_path: String,
    last_block_pos: u64,    
}

// read_at and write_at offset args should be valid as they checked in the VhdImage

macro_rules! debug_check {
    ($s:ident, $offset:ident, $data:ident) => {
        debug_assert!(($offset + $data.len() as u64) <= $s.file.size().unwrap() - crate::sizes::SECTOR_U64);
    };
}

impl ReadAt for FixedExtent {
    fn read_at(&self, offset: u64, data: &mut [u8]) -> Result<usize> {
        debug_check!(self, offset, data);

        self.file.read_at(offset, data)
    }
}

impl WriteAt for FixedExtent {
    fn write_at(&self, offset: u64, data: &[u8]) -> Result<usize> {
        debug_check!(self, offset, data);

        self.file.write_at(offset, data)
    }
}

impl Flush for FixedExtent {
    fn flush(&self) -> Result<()> {
        self.file.flush()
    }
}

impl SeekAt for FixedExtent {
    fn seek_at(&self, pos: std::io::SeekFrom) -> Result<u64> {
        self.file.seek_at(pos)
    }
}

impl ImageExtent for FixedExtent {
    fn backing_files(&self) -> Box<dyn Iterator<Item = String>> {
        Box::new(std::iter::once(self.file_path.clone()))
    }
    fn storage_size(&self) -> Result<u64> {
        self.file.size()
    }
}

impl ImageExtentOps for FixedExtent {}

impl VhdImageExtent for FixedExtent {
    fn write_footer(&self, footer: &VhdFooter) -> Result<()> {
        let bytes = footer.to_bytes();                     

        self.file.write_all_at(self.last_block_pos, &bytes)
    }

    fn sparse_header(&self) -> Option<&VhdHeader> {
        None
    }

    fn file_path(&self) -> String {
        self.file_path.clone()
    }

    fn parent_locator(&self) -> Option<String> {
        None
    }

    fn parent_locator_data(&self, index: usize) -> Option<Vec<u8>> {
        None
    }

    fn sparse_bat(&self) -> Option<&RefCell<bat::VhdBat>> {
        None
    }

    fn sparse_block_bitmap(&self, bat_block_index: usize) -> Option<(u64, &RefCell<Vec<u8>>)> {
        None
    }

    fn sparse_block_data(&self, bat_block_index: usize, buffer: &mut [u8]) -> Result<u64> {
        Ok(0)
    }
}

impl FixedExtent {
    fn new(file: VhdFile, file_path: String, last_block_pos: u64) -> Self {
        Self { file, file_path, last_block_pos }
    }    

    pub(crate) fn open(file: VhdFile, file_path: String) -> Result<Self> {
        let file_size = file.size()?;
        let last_block_pos = file_size - sizes::SECTOR_U64;
        
        Ok(Self::new(file, file_path, last_block_pos))
    }

    pub(crate) fn create(file_path: String, footer: &VhdFooter) -> Result<Self> {
        let file = VhdFile::create(&file_path, footer.current_size())?;

        let blks = footer.current_size() >> 21;
        // FIXME: write 2meg once
        let data = [0x00_u8; 4096];
        let mut pos = 0_u64;

        let data_count = (blks * DD_BLOCKSIZE_DEFAULT as u64) >> 12;        
        for _ in 0..data_count {
            file.write_all_at(pos, &data)?;
            pos += 4096;
        }        

        let this = Self::new(file, file_path, pos);
        this.write_footer(footer)?;        

        Ok(this)
    }
}
