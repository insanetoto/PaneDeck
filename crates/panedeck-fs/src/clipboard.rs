use std::{error::Error, fmt};

use panedeck_domain::NativePath;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileClipboardSnapshot {
    pub change_count: i64,
    pub paths: Vec<NativePath>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileClipboardError {
    Unsupported,
    InvalidPath,
    Platform,
}

impl fmt::Display for FileClipboardError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Unsupported => "system file clipboard is unsupported",
            Self::InvalidPath => "clipboard contains an invalid file path",
            Self::Platform => "system file clipboard operation failed",
        })
    }
}

impl Error for FileClipboardError {}

/// Operating-system boundary for interoperable file URL clipboard content.
pub trait FileClipboardBackend: Send + Sync {
    fn write_files(&self, paths: &[NativePath]) -> Result<i64, FileClipboardError>;
    fn read_files(&self) -> Result<FileClipboardSnapshot, FileClipboardError>;
}
