#[derive(Debug)]
pub enum VhdError {    
    ReadBeyondEOD,
    WriteBeyondEOD,
    UnexpectedEOD, //
    WriteZero,
    NotFound(String),

    FileTooSmall,
    InvalidHeaderCookie,
    InvalidHeaderChecksum,
    InvalidSparseHeaderCookie,
    InvalidSparseHeaderChecksum,
    InvalidSparseHeaderOffset,
    DiskSizeTooBig,
    UnknownVhdType(u32),
    InvalidBlockIndex(usize),
    UnexpectedBlockId(usize, u32), // the value returend from Bat::block_id()

    ParentNotExist,
    ParentNotDynamic,
    FilePathNeedAbsolute,
    CannotGetRelativePath, 
    NeedDyncOrDiffImage,   

    Io(std::io::Error),
}

impl core::fmt::Display for VhdError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            VhdError::ReadBeyondEOD => f.write_str("Read beyond EOD"),
            VhdError::WriteBeyondEOD => f.write_str("Write beyond EOD"),
            VhdError::UnexpectedEOD => f.write_str("Unexpected EOD"),
            VhdError::WriteZero => f.write_str("Write zero"),
            VhdError::NotFound(s) => write!(f, "Not found '{}'", s),

            VhdError::FileTooSmall => f.write_str("File too small"),
            VhdError::InvalidHeaderCookie => f.write_str("Invalid VHD header cookie"),
            VhdError::InvalidHeaderChecksum => f.write_str("Invalid VHD header checksum"),
            VhdError::InvalidSparseHeaderCookie => f.write_str("Invalid VHD Sparse header cookie"),
            VhdError::InvalidSparseHeaderChecksum => f.write_str("Invalid VHD Sparse header checksum"),
            VhdError::InvalidSparseHeaderOffset => f.write_str("Invalid VHD Sparse header BAT offset"),
            VhdError::DiskSizeTooBig => f.write_str("Disk size too big for VHD"),
            VhdError::UnknownVhdType(n) => write!(f, "Unknown VHD type '{}'", n),
            VhdError::InvalidBlockIndex(idx) => write!(f, "Invalid block index '{}'", idx),
            VhdError::UnexpectedBlockId(idx, id) => write!(f, "Unexpected '{}' block id '{:08X}'", idx, id),

            VhdError::ParentNotExist => f.write_str("Diff parent not exist"),
            VhdError::ParentNotDynamic => f.write_str("Diff parent not dynamic"),
            VhdError::FilePathNeedAbsolute => f.write_str("Need absolute file path"),
            VhdError::CannotGetRelativePath => f.write_str("Cannot get relative path"),
            VhdError::NeedDyncOrDiffImage => f.write_str("Need dynamic or diff type image"),
            
            VhdError::Io(e) => write!(f, "Io error: {}", e.to_string()),
        }
    }
}

impl std::error::Error for VhdError {}


impl From<std::io::Error> for VhdError {
    fn from(e: std::io::Error) -> Self {
        VhdError::Io(e)
    }
}