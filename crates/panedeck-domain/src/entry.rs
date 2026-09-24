use std::{fmt, time::SystemTime};

use crate::{DisplayName, EntryId, NativePath};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntryKind {
    File,
    Directory,
    Symlink,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SymlinkTarget {
    File,
    Directory,
    Other,
    Missing,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EntryMetadata {
    kind: EntryKind,
    byte_len: Option<u64>,
    modified_at: Option<SystemTime>,
    is_hidden: bool,
    is_read_only: bool,
    symlink_target: Option<SymlinkTarget>,
}

impl EntryMetadata {
    #[must_use]
    pub const fn new(kind: EntryKind) -> Self {
        Self {
            kind,
            byte_len: None,
            modified_at: None,
            is_hidden: false,
            is_read_only: false,
            symlink_target: None,
        }
    }

    #[must_use]
    pub const fn with_byte_len(mut self, byte_len: u64) -> Self {
        self.byte_len = Some(byte_len);
        self
    }

    #[must_use]
    pub const fn with_modified_at(mut self, modified_at: SystemTime) -> Self {
        self.modified_at = Some(modified_at);
        self
    }

    #[must_use]
    pub const fn with_hidden(mut self, is_hidden: bool) -> Self {
        self.is_hidden = is_hidden;
        self
    }

    #[must_use]
    pub const fn with_read_only(mut self, is_read_only: bool) -> Self {
        self.is_read_only = is_read_only;
        self
    }

    #[must_use]
    pub const fn with_symlink_target(mut self, target: SymlinkTarget) -> Self {
        self.symlink_target = Some(target);
        self
    }

    #[must_use]
    pub const fn kind(&self) -> EntryKind {
        self.kind
    }

    #[must_use]
    pub const fn byte_len(&self) -> Option<u64> {
        self.byte_len
    }

    #[must_use]
    pub const fn modified_at(&self) -> Option<SystemTime> {
        self.modified_at
    }

    #[must_use]
    pub const fn is_hidden(&self) -> bool {
        self.is_hidden
    }

    #[must_use]
    pub const fn is_read_only(&self) -> bool {
        self.is_read_only
    }

    #[must_use]
    pub const fn symlink_target(&self) -> Option<SymlinkTarget> {
        self.symlink_target
    }
}

/// One entry registered inside a [`crate::DirectorySession`].
pub struct FileEntry {
    id: EntryId,
    native_path: NativePath,
    display_name: DisplayName,
    metadata: EntryMetadata,
}

impl FileEntry {
    pub(crate) const fn new(
        id: EntryId,
        native_path: NativePath,
        display_name: DisplayName,
        metadata: EntryMetadata,
    ) -> Self {
        Self {
            id,
            native_path,
            display_name,
            metadata,
        }
    }

    #[must_use]
    pub const fn id(&self) -> EntryId {
        self.id
    }

    #[must_use]
    pub const fn native_path(&self) -> &NativePath {
        &self.native_path
    }

    #[must_use]
    pub const fn display_name(&self) -> &DisplayName {
        &self.display_name
    }

    #[must_use]
    pub const fn metadata(&self) -> &EntryMetadata {
        &self.metadata
    }
}

impl fmt::Debug for FileEntry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FileEntry")
            .field("id", &self.id)
            .field("native_path", &self.native_path)
            .field("display_name", &self.display_name)
            .field("metadata", &self.metadata)
            .finish()
    }
}
