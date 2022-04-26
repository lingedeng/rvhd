use std::io::SeekFrom;

use super::*;
use crate::{Uuid, math, Result, sizes, ReadAt, WriteAt, Flush, VhdError, Disk, DiskImage, Geometry, VhdFile, SeekAt};


pub use sparse::VhdHeader;

pub struct VhdImage {
    footer: VhdFooter,
    extent: Box<dyn VhdImageExtent>
}

impl Drop for VhdImage {
    fn drop(&mut self) {
        let res = self.flush();
        //debug_assert!(res.ok());
    }
}

impl ReadAt for VhdImage {
    fn read_at(&self, offset: u64, data: &mut [u8]) -> Result<usize> {
        println!("self.capacity: {}", self.capacity()?);
        match math::bound_to(self.capacity()?, offset, data.len()) {
            Some(data_len) => {
                println!("data_len: {}, offset: {}, buf len: {}", data_len, offset, data.len());
                self.extent.read_at(offset, &mut data[..data_len])
            },
            None => Err(VhdError::ReadBeyondEOD),
        }
    }
}

impl WriteAt for VhdImage {
    fn write_at(&self, offset: u64, data: &[u8]) -> Result<usize> {
        match math::bound_to(self.capacity()?, offset, data.len()) {
            Some(data_len) => self.extent.write_at(offset, &data[..data_len]),
            None => Err(VhdError::WriteBeyondEOD),
        }
    }
}

impl Flush for VhdImage {
    fn flush(&self) -> Result<()> {
        self.extent.write_footer(&self.footer)?;
        self.extent.flush()
    }
}

impl SeekAt for VhdImage {
    fn seek_at(&self, pos: std::io::SeekFrom) -> Result<u64> {
        self.extent.seek_at(pos)
    }
}

impl Disk for VhdImage {
    fn geometry(&self) -> Result<Geometry> {
        Ok(self.footer.geometry())
    }

    fn capacity(&self) -> Result<u64> {
        Ok(self.footer.current_size())
    }

    fn physical_sector_size(&self) -> Result<u32> {
        Ok(sizes::SECTOR)
    }
}

impl DiskImage for VhdImage {
    const NAME: &'static str = "VHD";
    const EXT: &'static [&'static str] = &["vhd"];

    fn backing_files(&self) -> Box<dyn std::iter::Iterator<Item = String>> {
        self.extent.backing_files()
    }

    fn storage_size(&self) -> Result<u64> {
        self.extent.storage_size()
    }
}

const MAX_VHD_SIZE: u64 = 2040 * sizes::GIB;
fn check_max_size(size: u64) -> Result<()> {
    if size > MAX_VHD_SIZE {
        return Err(VhdError::DiskSizeTooBig);
    }

    Ok(())
}

impl VhdImage {
    pub fn create_fixed<S: Into<String>>(path: S, size_mb: u64) -> Result<Self> {        
        let size = size_mb << 20;
        let blks = math::ceil(size, DD_BLOCKSIZE_DEFAULT as u64) as u64;
        let size = blks << 21;        

        check_max_size(size)?;

        let path = path.into();               
        let footer = VhdFooter::new(size, VhdType::Fixed);
        let extent: Box<dyn VhdImageExtent> = Box::new(FixedExtent::create(path, &footer)?);             

        Ok(VhdImage {
            footer,
            extent,
        })
    }

    pub fn create_dynamic<S: Into<String>>(path: S, size_mb: u64) -> Result<Self> {
        let size = size_mb << 20;
        let blks = math::ceil(size, DD_BLOCKSIZE_DEFAULT as u64) as u64;
        let size = blks << 21;

        check_max_size(size)?;

        let path = path.into();
        let footer = VhdFooter::new(size, VhdType::Dynamic);
        let extent: Box<dyn VhdImageExtent> = Box::new(SparseExtent::create(path, &footer, None)?);

        Ok(VhdImage {
            footer,
            extent,
        })
    }

    pub fn create_diff<S: Into<String>>(path: S, parent: S) -> Result<Self> {
        let path = path.into();
        let parent_path = parent.into();

        if !std::path::Path::new(&parent_path).exists() {
            return Err(VhdError::ParentNotExist);
        }

        let parent_img = Self::open(parent_path)?;
        match parent_img.disk_type() {
            VhdType::Fixed => return Err(VhdError::ParentNotDynamic),
            _ => (),
        };

        let size = parent_img.capacity()?;
        let footer = VhdFooter::new(size, VhdType::Diff);
        let extent: Box<dyn VhdImageExtent> = Box::new(SparseExtent::create(path, &footer, Some(parent_img))?);

        Ok(VhdImage {
            footer,
            extent,
        })
    }
    
    pub fn open<S: Into<String>>(path: S) -> Result<Self> {
        let path = path.into();
        let file = VhdFile::open(&path)?;
        let file_size = file.size()?;

        if file_size < sizes::SECTOR_U64 {
            return Err(VhdError::FileTooSmall);
        }

        let footer_pos = file_size - sizes::SECTOR_U64;
        let footer = VhdFooter::read(&file, footer_pos)?;
        // Note: Versions previous to Microsoft Virtual PC 2004 create disk images that have a 511-byte disk footer.
        // So the hard disk footer can exist in the last 511 or 512 bytes of the file that holds the hard disk image.
        // At the moment rdisk does not support files with 511-bytes footer.

        let extent: Box<dyn VhdImageExtent> = match footer.disk_type() {
            VhdType::Fixed => Box::new(FixedExtent::open(file, path)?),
            VhdType::Dynamic | VhdType::Diff => Box::new(SparseExtent::open(file, path, footer.data_offset())?),
        };

        Ok(Self { footer, extent })
    }    
}

impl VhdImage {
    pub fn disk_type(&self) -> VhdType {
        self.footer.disk_type()
    }

    pub fn id(&self) -> &Uuid {
        self.footer.uuid()
    }

    pub fn footer(&self) -> &VhdFooter {
        &self.footer
    }

    pub fn sparse_header(&self) -> Option<&VhdHeader> {
        self.extent.sparse_header()
    }

    pub fn file_path(&self) -> String {
        self.extent.file_path()
    }

    pub fn parent_locator(&self) -> Option<String> {
        self.extent.parent_locator()
    }    

    pub fn file_size(&self) -> Result<u64> {
        self.extent.storage_size()
    }

    pub fn parent_locator_data(&self, index: usize) -> Option<Vec<u8>> {
        self.extent.parent_locator_data(index)
    }

    pub fn sparse_bat(&self) -> Option<&RefCell<bat::VhdBat>> {
        self.extent.sparse_bat()
    }

    pub fn sparse_block_bitmap(&self, bat_block_index: usize) -> Option<(u64, &RefCell<Vec<u8>>)> {
        self.extent.sparse_block_bitmap(bat_block_index)
    }

    pub fn sparse_block_data(&self, bat_block_index: usize, buffer: &mut [u8]) -> Result<u64> {
        self.extent.sparse_block_data(bat_block_index, buffer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_fixed_test() {
        let vhd_fixed = VhdImage::create_fixed("D:\\123.vhd", 10).unwrap();        
        assert_eq!(vhd_fixed.disk_type(), VhdType::Fixed);        
    }

    #[test]
    fn open_fixed_test() {
        let vhd_fixed = VhdImage::open("D:\\123.vhd").unwrap();
        assert_eq!(vhd_fixed.disk_type(), VhdType::Fixed);
        assert_eq!(vhd_fixed.footer().current_size(), 10 << 20);

        println!("{}", vhd_fixed.footer());
    }

    #[test]
    fn create_dynamic_test() {
        let vhd_dynamic = VhdImage::create_dynamic("D:\\456.vhd", 2).unwrap();
        assert_eq!(vhd_dynamic.disk_type(), VhdType::Dynamic);
        assert_eq!(vhd_dynamic.footer().current_size(), 2 << 20);
    }

    #[test]
    fn open_dynamic_test() {
        let vhd_dynamic = VhdImage::open("D:\\456.vhd").unwrap();
        assert_eq!(vhd_dynamic.disk_type(), VhdType::Dynamic);
        assert_eq!(vhd_dynamic.footer().current_size(), 2 << 20);        

        println!("{}", vhd_dynamic.footer());
        println!("{}", vhd_dynamic.sparse_header().as_deref().unwrap());
    }

    #[test]
    fn create_diff_test() {
        let vhd_diff = VhdImage::create_diff("D:\\567.vhd", "D:\\456.vhd").unwrap();
        assert_eq!(vhd_diff.footer().current_size(), 2 << 20);
    }

    #[test]
    fn open_diff_test() {
        let vhd_diff = VhdImage::open("D:\\567.vhd").unwrap();
        assert_eq!(vhd_diff.disk_type(), VhdType::Diff);
        assert_eq!(vhd_diff.footer().current_size(), 2 << 20);        

        println!("{}", vhd_diff.footer());
        println!("{}", vhd_diff.sparse_header().as_deref().unwrap());
        println!("{}", vhd_diff.parent_locator().unwrap());
    }
}