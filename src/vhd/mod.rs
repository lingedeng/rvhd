use std::time::{SystemTime, UNIX_EPOCH, Duration};
use crate::{AsByteSlice, ImageExtent, ImageExtentOps, Result};
use std::cell::{RefCell, Ref};

pub(crate) fn calc_header_bytes_checksum<T: AsByteSlice>(header: &T) -> u32 {
    let mut new_checksum = 0_u32;
    for b in unsafe { header.as_byte_slice() } {
        new_checksum += *b as u32;
    }

    !new_checksum
}

macro_rules!  calc_header_checksum {
    ($header:ident) => {{
        let mut copied = $header.clone();
        copied.checksum = 0;

        crate::vhd::calc_header_bytes_checksum(&copied)
    }};
}

pub(crate) fn vhd_time() -> u32 {
    let sys_time = SystemTime::now();    

    if let Ok(dur) = sys_time.duration_since(UNIX_EPOCH) {
        dur.as_secs() as u32 - VHD_EPOCH_START
    } else {
        0
    }
}

// FIXME: need third-part crate
pub(crate) fn vhd_time_str(time: u32) -> Result<()> {
    let _sys_time = SystemTime::now();    
    let _dur = Duration::from_secs(time as u64 + VHD_EPOCH_START as u64);

    Ok(())
}

pub(crate) fn vhd_type_str(vhd_type: VhdType) -> String {
    match vhd_type {
        VhdType::Fixed => String::from("Fixed"),
        VhdType::Dynamic => String::from("Dynamic"),
        VhdType::Diff => String::from("Differencing"),
    }
}

/// VHD uses an epoch of 12:00AM, Jan 1, 2000. This is the Unix timestamp for the start of the VHD epoch.
const VHD_EPOCH_START: u32 = 9_4668_4800;
pub const DEFAULT_HEADER_OFFSET: u64 = std::mem::size_of::<VhdFooter>() as u64;
pub const DEFAULT_TABLE_OFFSET: u64 = DEFAULT_HEADER_OFFSET + std::mem::size_of::<VhdHeader>() as u64;

pub mod footer;
pub use footer::*;

pub mod image;
pub use image::*;

pub mod fixed;
pub use fixed::*;

pub mod sparse;
pub use sparse::*;

pub mod journal;
pub use journal::*;

trait VhdImageExtent: ImageExtent + ImageExtentOps {
    fn write_footer(&self, footer: &VhdFooter) -> Result<()>;
    fn sparse_header(&self) -> Option<&VhdHeader>;
    fn file_path(&self) -> String;
    fn parent_locator(&self) -> Option<String>;
    fn parent_locator_data(&self, index: usize) -> Option<Vec<u8>>;
    fn sparse_bat(&self) -> Option<&RefCell<bat::VhdBat>>;
    fn sparse_block_bitmap(&self, bat_block_index: usize) -> Option<(u64, &RefCell<Vec<u8>>)>;
    fn sparse_block_data(&self, bat_block_index: usize, buffer: &mut [u8]) -> Result<u64>;
}

#[derive(Debug, Copy, Clone, FromPrimitive, ToPrimitive, Eq, PartialEq)]
pub enum VhdType {
    Fixed = 2,
    Dynamic = 3,
    Diff = 4,
}