use std::{error::Error, fmt, time::UNIX_EPOCH};

use panedeck_domain::{
    DirectoryReference, DirectorySessionId, EntryId, EntryKind, EntryReference, FileEntry,
    SymlinkTarget,
};
use panedeck_fs::DirectoryEntrySummary;
use serde::{Deserialize, Serialize};

/// JSON-safe directory capability. It contains no native path.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DirectoryReferenceDto {
    pub session_id: String,
}

impl From<DirectoryReference> for DirectoryReferenceDto {
    fn from(value: DirectoryReference) -> Self {
        Self {
            session_id: value.session_id().get().to_string(),
        }
    }
}

impl TryFrom<DirectoryReferenceDto> for DirectoryReference {
    type Error = IpcReferenceError;

    fn try_from(value: DirectoryReferenceDto) -> Result<Self, Self::Error> {
        let session_id = parse_session_id(&value.session_id)?;
        Ok(Self::new(session_id))
    }
}

/// JSON-safe entry capability. Both identifiers must resolve in one live
/// server-owned [`panedeck_domain::DirectorySession`].
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EntryReferenceDto {
    pub session_id: String,
    pub entry_id: String,
}

impl From<EntryReference> for EntryReferenceDto {
    fn from(value: EntryReference) -> Self {
        Self {
            session_id: value.session_id().get().to_string(),
            entry_id: value.entry_id().get().to_string(),
        }
    }
}

impl TryFrom<EntryReferenceDto> for EntryReference {
    type Error = IpcReferenceError;

    fn try_from(value: EntryReferenceDto) -> Result<Self, Self::Error> {
        Ok(Self::new(
            parse_session_id(&value.session_id)?,
            parse_entry_id(&value.entry_id)?,
        ))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EntryKindDto {
    File,
    Directory,
    Symlink,
    Other,
}

impl From<EntryKind> for EntryKindDto {
    fn from(value: EntryKind) -> Self {
        match value {
            EntryKind::File => Self::File,
            EntryKind::Directory => Self::Directory,
            EntryKind::Symlink => Self::Symlink,
            EntryKind::Other => Self::Other,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SymlinkTargetDto {
    File,
    Directory,
    Other,
    Missing,
    Unknown,
}

impl From<SymlinkTarget> for SymlinkTargetDto {
    fn from(value: SymlinkTarget) -> Self {
        match value {
            SymlinkTarget::File => Self::File,
            SymlinkTarget::Directory => Self::Directory,
            SymlinkTarget::Other => Self::Other,
            SymlinkTarget::Missing => Self::Missing,
            SymlinkTarget::Unknown => Self::Unknown,
        }
    }
}

/// Read-only snapshot sent to the UI. Sizes and timestamps are decimal strings
/// so JavaScript cannot lose precision above `Number.MAX_SAFE_INTEGER`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntrySnapshotDto {
    pub reference: EntryReferenceDto,
    pub display_name: String,
    pub display_name_is_lossy: bool,
    pub kind: EntryKindDto,
    pub byte_len: Option<String>,
    pub modified_at_unix_millis: Option<String>,
    pub is_hidden: bool,
    pub is_read_only: bool,
    pub symlink_target: Option<SymlinkTargetDto>,
}

impl EntrySnapshotDto {
    #[must_use]
    pub fn from_entry(session: DirectoryReference, entry: &FileEntry) -> Self {
        let metadata = entry.metadata();
        let modified_at_unix_millis = metadata
            .modified_at()
            .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis().to_string());

        Self {
            reference: EntryReference::new(session.session_id(), entry.id()).into(),
            display_name: entry.display_name().as_str().to_owned(),
            display_name_is_lossy: entry.display_name().is_lossy(),
            kind: metadata.kind().into(),
            byte_len: metadata.byte_len().map(|value| value.to_string()),
            modified_at_unix_millis,
            is_hidden: metadata.is_hidden(),
            is_read_only: metadata.is_read_only(),
            symlink_target: metadata.symlink_target().map(Into::into),
        }
    }

    #[must_use]
    pub fn from_summary(summary: &DirectoryEntrySummary) -> Self {
        let metadata = &summary.metadata;
        let modified_at_unix_millis = metadata
            .modified_at()
            .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis().to_string());
        Self {
            reference: summary.reference.into(),
            display_name: summary.display_name.clone(),
            display_name_is_lossy: summary.display_name_is_lossy,
            kind: metadata.kind().into(),
            byte_len: metadata.byte_len().map(|value| value.to_string()),
            modified_at_unix_millis,
            is_hidden: metadata.is_hidden(),
            is_read_only: metadata.is_read_only(),
            symlink_target: metadata.symlink_target().map(Into::into),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IpcReferenceError {
    InvalidSessionId,
    InvalidEntryId,
}

impl fmt::Display for IpcReferenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidSessionId => "invalid directory session reference",
            Self::InvalidEntryId => "invalid directory entry reference",
        })
    }
}

impl Error for IpcReferenceError {}

fn parse_session_id(value: &str) -> Result<DirectorySessionId, IpcReferenceError> {
    value
        .parse::<u64>()
        .ok()
        .and_then(DirectorySessionId::from_raw)
        .ok_or(IpcReferenceError::InvalidSessionId)
}

fn parse_entry_id(value: &str) -> Result<EntryId, IpcReferenceError> {
    value
        .parse::<u64>()
        .ok()
        .and_then(EntryId::from_raw)
        .ok_or(IpcReferenceError::InvalidEntryId)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use panedeck_domain::{DirectorySession, EntryMetadata, NativePath, ResolveReferenceError};

    use super::*;

    fn session_id(value: u64) -> DirectorySessionId {
        DirectorySessionId::from_raw(value).expect("test session ids are non-zero")
    }

    #[test]
    fn rejects_path_fields_and_invalid_ids_from_json() {
        let injected_path = r#"{"sessionId":"1","entryId":"1","nativePath":"/tmp/escape"}"#;
        assert!(serde_json::from_str::<EntryReferenceDto>(injected_path).is_err());

        let invalid_id = EntryReferenceDto {
            session_id: "1".to_owned(),
            entry_id: "../../escape".to_owned(),
        };
        assert_eq!(
            EntryReference::try_from(invalid_id).unwrap_err(),
            IpcReferenceError::InvalidEntryId
        );
    }

    #[test]
    fn forged_but_well_formed_ids_do_not_resolve() {
        let session = DirectorySession::new(
            session_id(4),
            NativePath::new(PathBuf::from("/tmp/panedeck-ipc")),
        )
        .unwrap();
        let forged = EntryReference::try_from(EntryReferenceDto {
            session_id: "4".to_owned(),
            entry_id: "999".to_owned(),
        })
        .unwrap();

        assert_eq!(
            session.resolve(forged).unwrap_err(),
            ResolveReferenceError::UnknownEntry
        );
    }

    #[cfg(unix)]
    #[test]
    fn json_reference_round_trip_preserves_non_utf8_native_path() {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt};

        let root = PathBuf::from("/tmp/panedeck-ipc");
        let native_name = OsString::from_vec(b"asset-\xff.raw".to_vec());
        let expected_path = root.join(native_name);
        let mut session = DirectorySession::new(session_id(11), NativePath::new(root)).unwrap();
        let reference = session
            .register_entry(
                NativePath::new(expected_path.clone()),
                EntryMetadata::new(EntryKind::File).with_byte_len(u64::MAX),
            )
            .unwrap();

        let json = serde_json::to_string(&EntryReferenceDto::from(reference)).unwrap();
        assert!(!json.contains("/tmp"));
        assert!(!json.contains("nativePath"));
        let round_tripped_dto: EntryReferenceDto = serde_json::from_str(&json).unwrap();
        let round_tripped_reference = EntryReference::try_from(round_tripped_dto).unwrap();
        let entry = session.resolve(round_tripped_reference).unwrap();

        assert_eq!(entry.native_path().as_path(), expected_path);
        assert!(entry.display_name().is_lossy());

        let snapshot = EntrySnapshotDto::from_entry(session.reference(), entry);
        assert_eq!(snapshot.byte_len.as_deref(), Some("18446744073709551615"));
        assert!(snapshot.display_name_is_lossy);
    }
}
