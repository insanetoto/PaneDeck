//! Platform-independent domain vocabulary for PaneDeck.
//!
//! This crate deliberately has no workspace dependencies. File-system,
//! persistence, operating-system, and UI concerns belong in outer layers.

mod entry;
mod error;
mod id;
mod path;
mod session;

pub use entry::{EntryKind, EntryMetadata, FileEntry, SymlinkTarget};
pub use error::{AccessErrorKind, AccessOperation, AccessSubject, FileAccessError};
pub use id::{DirectoryReference, DirectorySessionId, EntryId, EntryReference};
pub use path::{DisplayName, NativePath};
pub use session::{DirectorySession, ResolveReferenceError, SessionModelError};

/// Stable product name shared by outer application layers.
pub const PRODUCT_NAME: &str = "PaneDeck";

/// Marker for the platform-independent domain layer.
#[derive(Debug, Default, Eq, PartialEq)]
pub struct DomainLayer;

#[cfg(test)]
mod tests {
    use super::PRODUCT_NAME;

    #[test]
    fn product_name_is_stable() {
        assert_eq!(PRODUCT_NAME, "PaneDeck");
    }
}
