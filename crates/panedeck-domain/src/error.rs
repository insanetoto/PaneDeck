use std::{error::Error, fmt};

use crate::{DirectoryReference, EntryReference};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccessOperation {
    OpenDirectory,
    ReadEntry,
    ReadMetadata,
    ReadSymlink,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccessErrorKind {
    NotFound,
    PermissionDenied,
    NotDirectory,
    InvalidPath,
    UnsupportedFileType,
    Io,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccessSubject {
    Directory(DirectoryReference),
    Entry(EntryReference),
    Unresolved,
}

/// Path-redacted error suitable for mapping to IPC error codes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FileAccessError {
    operation: AccessOperation,
    kind: AccessErrorKind,
    subject: AccessSubject,
}

impl FileAccessError {
    #[must_use]
    pub const fn new(
        operation: AccessOperation,
        kind: AccessErrorKind,
        subject: AccessSubject,
    ) -> Self {
        Self {
            operation,
            kind,
            subject,
        }
    }

    #[must_use]
    pub const fn operation(self) -> AccessOperation {
        self.operation
    }

    #[must_use]
    pub const fn kind(self) -> AccessErrorKind {
        self.kind
    }

    #[must_use]
    pub const fn subject(self) -> AccessSubject {
        self.subject
    }
}

impl fmt::Display for FileAccessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:?} failed: {:?}", self.operation, self.kind)
    }
}

impl Error for FileAccessError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DirectorySessionId, EntryId, EntryReference};

    fn entry_reference() -> EntryReference {
        EntryReference::new(
            DirectorySessionId::from_raw(1).unwrap(),
            EntryId::from_raw(2).unwrap(),
        )
    }

    #[test]
    fn represents_permission_denied_without_a_native_path() {
        let error = FileAccessError::new(
            AccessOperation::ReadMetadata,
            AccessErrorKind::PermissionDenied,
            AccessSubject::Entry(entry_reference()),
        );

        assert_eq!(error.kind(), AccessErrorKind::PermissionDenied);
        assert!(!format!("{error:?}").contains('/'));
    }

    #[test]
    fn represents_an_entry_that_disappeared() {
        let error = FileAccessError::new(
            AccessOperation::ReadEntry,
            AccessErrorKind::NotFound,
            AccessSubject::Entry(entry_reference()),
        );

        assert_eq!(error.kind(), AccessErrorKind::NotFound);
    }
}
