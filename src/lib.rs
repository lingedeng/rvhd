/// Declaring or defining a new module can be thought of as inserting a new tree into the hierarchy at the location of the definition.
/// By default, everything in Rust is private, with two exceptions: 
/// 1. Associated items in a pub Trait are public by default; 
/// 2. Enum variants in a pub enum are also public by default.
#[macro_use]
extern crate num_derive;

mod error;
pub use error::VhdError;
pub type Result<T> = std::result::Result<T, VhdError>;

pub use uuid::Uuid;

mod traits;
pub use traits::*;

mod util;
pub use util::*;

mod geometry;
pub use geometry::*;

mod math;
pub use math::*;

mod vhd;

trait UuidEx {
    fn swap_bytes(&self) -> Self;
    fn from_be_bytes(bytes: [u8; 16]) -> Self;
    fn from_le_bytes(bytes: [u8; 16]) -> Self;
}

impl UuidEx for Uuid {
    fn swap_bytes(&self) -> Self {
        let fields = self.to_fields_le();
        Uuid::from_fields(fields.0, fields.1, fields.2, fields.3).unwrap()
    }

    fn from_be_bytes(bytes: [u8; 16]) -> Self {
        Uuid::from_bytes(bytes).swap_bytes()
    }

    fn from_le_bytes(bytes: [u8; 16]) -> Self {
        Uuid::from_bytes(bytes)
    }
}

pub mod sizes {
    pub const SECTOR: u32 = 512;
    pub const SECTOR_U64: u64 = SECTOR as u64;
    pub const KIB: u64 = 1024;
    pub const MIB: u64 = 1024 * KIB;
    pub const GIB: u64 = 1024 * MIB;
    pub const SECTOR_SHIFT: u32 = 9;
}

/* Layout of a dynamic disk:
 *
 * +-------------------------------------------------+
 * | Mirror image of HD footer (hd_ftr) (512 bytes)  |
 * +-------------------------------------------------+
 * | Sparse drive header (dd_hdr) (1024 bytes)       |
 * +-------------------------------------------------+
 * | BAT (Block allocation table)                    |
 * |   - Array of absolute sector offsets into the   |
 * |     file (u32).                                 |
 * |   - Rounded up to a sector boundary.            |
 * |   - Unused entries are marked as 0xFFFFFFFF     |
 * |   - max entries in dd_hdr->max_bat_size         |
 * +-------------------------------------------------+
 * | Data Block 0                                    |
 * | Bitmap (padded to 512 byte sector boundary)     |
 * |   - each bit indicates whether the associated   |
 * |     sector within this block is used.           |
 * | Data                                            |
 * |   - power-of-two multiple of sectors.           |
 * |   - default 2MB (4096 * 512)                    |
 * |   - Any entries with zero in bitmap should be   |
 * |     zero on disk                                |
 * +-------------------------------------------------+
 * | Data Block 1                                    |
 * +-------------------------------------------------+
 * | ...                                             |
 * +-------------------------------------------------+
 * | Data Block n                                    |
 * +-------------------------------------------------+
 * | HD Footer (511 bytes)                           |
 * +-------------------------------------------------+
 */
