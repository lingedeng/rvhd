use crate::sizes;

#[derive(Debug, Copy, Clone)]
pub struct Geometry {
    pub cylinders: u64,
    pub heads: u32,
    pub sectors_per_track: u32,
    pub bytes_per_sector: u32,
}

impl std::fmt::Display for Geometry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.bytes_per_sector == sizes::SECTOR {
            write!(f, "({}/{}/{})", self.cylinders, self.heads, self.sectors_per_track)
        } else {
            write!(f, "({}/{}/{}:{})", self.cylinders, self.heads, self.sectors_per_track, self.bytes_per_sector)
        }
    }
}

impl Geometry {
    pub fn chs(cylinders: u64, heads: u32, sectors_per_track: u32) -> Self {
        Geometry {
            cylinders,
            heads,
            sectors_per_track,
            bytes_per_sector: sizes::SECTOR,
        }
    }

    pub fn with_vhd_capacity(capacity: u64) -> Self {
        Self::with_vhd_capacity_and_sector(capacity, sizes::SECTOR)
    }

    pub fn with_vhd_capacity_and_sector(capacity: u64, sector_size: u32) -> Self {
        //                                      Cylinders   Heads    Sectors
        let total_sectors = if capacity > 65535_u64 * 16_u64 * 255_u64 * sector_size as u64 {
            65535_u32 * 16_u32 * 255_u32
        } else {
            capacity as u32 / sector_size
        };
    
        let (heads_per_cylinder, sectors_per_track) = if total_sectors > 65535_u32 * 16_u32 * 63_u32 {
            (255, 16)
        } else {
            let mut sectors_per_track = 17_u32;
            let mut cylinders_times_heads = total_sectors / sectors_per_track;
            let mut heads_per_cylinder = (cylinders_times_heads + 1023) / 1024;
    
            if heads_per_cylinder < 4 {
                heads_per_cylinder = 4
            }
    
            if cylinders_times_heads >= heads_per_cylinder * 1024 || heads_per_cylinder > 16 {
                sectors_per_track = 31;
                heads_per_cylinder = 16;
                cylinders_times_heads = total_sectors / sectors_per_track;
            }
    
            if cylinders_times_heads >= heads_per_cylinder * 1024 {
                sectors_per_track = 63;
                heads_per_cylinder = 16;
            }
    
            (heads_per_cylinder, sectors_per_track)
        };
    
        let cylinders = total_sectors / sectors_per_track / heads_per_cylinder;
    
        Geometry {
            cylinders: cylinders as u64,
            heads: heads_per_cylinder,
            sectors_per_track: sectors_per_track,
            bytes_per_sector: sector_size,
        }
    }

    pub fn capacity(&self) -> u64 {
        self.capacity_in_sectors() * (self.bytes_per_sector as u64)
    }

    pub fn capacity_in_sectors(&self) -> u64 {
        self.cylinders * (self.heads as u64) * (self.sectors_per_track as u64)
    }
}