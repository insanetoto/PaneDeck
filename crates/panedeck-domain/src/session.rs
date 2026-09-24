use std::{collections::BTreeMap, error::Error, fmt};

use crate::{
    DirectoryReference, DirectorySessionId, DisplayName, EntryId, EntryMetadata, EntryReference,
    FileEntry, NativePath,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionModelError {
    RootMustBeAbsolute,
    EntryMustBeDirectChild,
    EntryHasNoFileName,
    DuplicateEntry,
    EntryIdSpaceExhausted,
}

impl fmt::Display for SessionModelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::RootMustBeAbsolute => "directory session root must be absolute",
            Self::EntryMustBeDirectChild => "entry must be a direct child of the session root",
            Self::EntryHasNoFileName => "entry path has no file name",
            Self::DuplicateEntry => "entry is already registered in this session",
            Self::EntryIdSpaceExhausted => "directory session entry id space is exhausted",
        })
    }
}

impl Error for SessionModelError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResolveReferenceError {
    ForeignSession,
    UnknownEntry,
}

impl fmt::Display for ResolveReferenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ForeignSession => "entry reference belongs to another directory session",
            Self::UnknownEntry => "entry reference is not registered in this directory session",
        })
    }
}

impl Error for ResolveReferenceError {}

/// Server-owned mapping from opaque references to native paths.
///
/// The mapping is never serialized. IPC consumers can only send references
/// back to the application, which must resolve them through this session.
pub struct DirectorySession {
    id: DirectorySessionId,
    root: NativePath,
    entries: BTreeMap<EntryId, FileEntry>,
    next_entry_id: u64,
}

impl DirectorySession {
    pub fn new(id: DirectorySessionId, root: NativePath) -> Result<Self, SessionModelError> {
        if !root.as_path().is_absolute() {
            return Err(SessionModelError::RootMustBeAbsolute);
        }

        Ok(Self {
            id,
            root,
            entries: BTreeMap::new(),
            next_entry_id: 1,
        })
    }

    #[must_use]
    pub const fn reference(&self) -> DirectoryReference {
        DirectoryReference::new(self.id)
    }

    #[must_use]
    pub const fn native_root(&self) -> &NativePath {
        &self.root
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn register_entry(
        &mut self,
        native_path: NativePath,
        metadata: EntryMetadata,
    ) -> Result<EntryReference, SessionModelError> {
        if native_path.as_path().parent() != Some(self.root.as_path()) {
            return Err(SessionModelError::EntryMustBeDirectChild);
        }

        if self
            .entries
            .values()
            .any(|entry| entry.native_path() == &native_path)
        {
            return Err(SessionModelError::DuplicateEntry);
        }

        let file_name = native_path
            .as_path()
            .file_name()
            .ok_or(SessionModelError::EntryHasNoFileName)?;
        let display_name = DisplayName::from_os_str(file_name);
        let entry_id = EntryId::from_raw(self.next_entry_id)
            .ok_or(SessionModelError::EntryIdSpaceExhausted)?;
        self.next_entry_id = self
            .next_entry_id
            .checked_add(1)
            .ok_or(SessionModelError::EntryIdSpaceExhausted)?;

        self.entries.insert(
            entry_id,
            FileEntry::new(entry_id, native_path, display_name, metadata),
        );
        Ok(EntryReference::new(self.id, entry_id))
    }

    pub fn resolve(&self, reference: EntryReference) -> Result<&FileEntry, ResolveReferenceError> {
        if reference.session_id() != self.id {
            return Err(ResolveReferenceError::ForeignSession);
        }

        self.entries
            .get(&reference.entry_id())
            .ok_or(ResolveReferenceError::UnknownEntry)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::{EntryKind, SymlinkTarget};

    fn session_id(value: u64) -> DirectorySessionId {
        DirectorySessionId::from_raw(value).expect("test session ids are non-zero")
    }

    #[test]
    fn rejects_relative_roots_and_non_child_entries() {
        assert_eq!(
            DirectorySession::new(session_id(1), NativePath::new("relative"))
                .err()
                .expect("relative roots must fail"),
            SessionModelError::RootMustBeAbsolute
        );

        let mut session = DirectorySession::new(
            session_id(1),
            NativePath::new(PathBuf::from("/tmp/panedeck-model")),
        )
        .expect("absolute root is valid");
        let result = session.register_entry(
            NativePath::new("/tmp/outside"),
            EntryMetadata::new(EntryKind::File),
        );
        assert_eq!(
            result.expect_err("outside entries must fail"),
            SessionModelError::EntryMustBeDirectChild
        );
    }

    #[test]
    fn models_hidden_read_only_and_symlink_entries() {
        let root = PathBuf::from("/tmp/panedeck-model");
        let mut session =
            DirectorySession::new(session_id(1), NativePath::new(root.clone())).unwrap();
        let reference = session
            .register_entry(
                NativePath::new(root.join(".linked")),
                EntryMetadata::new(EntryKind::Symlink)
                    .with_hidden(true)
                    .with_read_only(true)
                    .with_symlink_target(SymlinkTarget::Missing),
            )
            .unwrap();
        let entry = session.resolve(reference).unwrap();

        assert_eq!(entry.metadata().kind(), EntryKind::Symlink);
        assert_eq!(
            entry.metadata().symlink_target(),
            Some(SymlinkTarget::Missing)
        );
        assert!(entry.metadata().is_hidden());
        assert!(entry.metadata().is_read_only());
    }

    #[test]
    fn rejects_foreign_and_unknown_references() {
        let session = DirectorySession::new(
            session_id(7),
            NativePath::new(PathBuf::from("/tmp/panedeck-model")),
        )
        .unwrap();

        let foreign = EntryReference::new(
            session_id(8),
            EntryId::from_raw(1).expect("non-zero entry id"),
        );
        assert_eq!(
            session.resolve(foreign).expect_err("foreign id must fail"),
            ResolveReferenceError::ForeignSession
        );

        let unknown = EntryReference::new(
            session_id(7),
            EntryId::from_raw(99).expect("non-zero entry id"),
        );
        assert_eq!(
            session.resolve(unknown).expect_err("unknown id must fail"),
            ResolveReferenceError::UnknownEntry
        );
    }

    #[cfg(unix)]
    #[test]
    fn preserves_non_utf8_native_names() {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};

        let root = PathBuf::from("/tmp/panedeck-model");
        let native_name = std::ffi::OsString::from_vec(b"report-\xff.bin".to_vec());
        let native_path = root.join(&native_name);
        let mut session = DirectorySession::new(session_id(1), NativePath::new(root)).unwrap();
        let reference = session
            .register_entry(
                NativePath::new(native_path),
                EntryMetadata::new(EntryKind::File).with_byte_len(42),
            )
            .unwrap();
        let entry = session.resolve(reference).unwrap();

        assert_eq!(
            entry
                .native_path()
                .as_path()
                .file_name()
                .unwrap()
                .as_bytes(),
            native_name.as_bytes()
        );
        assert!(entry.display_name().is_lossy());
    }
}
