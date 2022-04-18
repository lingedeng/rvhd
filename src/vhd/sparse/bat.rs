use crate::traits::{ReadAt, WriteAt};
use crate::{Result, VhdError, math, sizes};
use crate::util::AsByteSlice;

#[repr(C, packed)]
pub struct VhdBat {
    // sector number per block
    //spb: u32,
    // total entry number
    //entries: u32,
    //entry table
    bat: Vec<u32>,
}

const DD_BLOCK_UNUSED: u32 = 0xFFFF_FFFF;

impl VhdBat {
    pub fn new(entries: u32) -> Self {
        VhdBat { 
            bat: vec![DD_BLOCK_UNUSED; entries as usize],
        }
    }

    pub fn read(stream: &impl ReadAt, offset: u64, entries: u32) -> Result<Self> {
        let entries = entries as usize;

        let mut table = VhdBat { 
            bat: Vec::with_capacity(entries),
        };

        let buffer = unsafe {
            table.bat.set_len(entries);
            std::slice::from_raw_parts_mut(table.bat.as_mut_ptr() as *mut u8, entries*4)
        };

        stream.read_exact_at(offset, buffer)?;

        for entry in &mut table.bat {
            *entry = entry.swap_bytes();
        }

        Ok(table)
    }

    pub fn write(&self, stream: &impl WriteAt, offset: u64) -> Result<usize> {
        let mut tmp = self.bat.clone();
        for entry in &mut tmp {
            *entry = entry.swap_bytes();
        }

        // The BAT is always extended to a sector boundary.
        let size = math::round_up(self.bat.len() * 4, sizes::SECTOR as usize);
        let mut buffer = vec![0xFF_u8; size];
        let data = unsafe { tmp.as_byte_slice() };
        buffer[..data.len()].copy_from_slice(data);

        stream.write_all_at(offset, &buffer);

        Ok(buffer.len())
    }

    pub fn block_id(&self, index: usize) -> Result<u32> {
        match self.bat.get(index) {
            Some(id) => Ok(*id),
            None => return Err(VhdError::InvalidBlockIndex(index)),
        }
    }

    /// The `index` MUST always be valid!
    pub fn set_block_id(&mut self, index: usize, id: u32) -> Result<()> {
        if index < self.bat.len() {
            return Err(VhdError::InvalidBlockIndex(index));
        }

        self.bat[index] = id;

        Ok(())
    }
}