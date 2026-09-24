use std::{borrow::Cow, ffi::OsStr, fmt, path::PathBuf};

/// An operating-system-native path that is never serialized across IPC.
///
/// Keeping the [`PathBuf`] intact prevents non-UTF-8 names from being changed
/// by a lossy JSON string round trip. Only opaque references may leave Rust.
#[derive(Clone, Eq, Hash, PartialEq)]
pub struct NativePath(PathBuf);

impl NativePath {
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self(path.into())
    }

    #[must_use]
    pub fn as_path(&self) -> &std::path::Path {
        &self.0
    }

    #[must_use]
    pub fn into_path_buf(self) -> PathBuf {
        self.0
    }
}

impl fmt::Debug for NativePath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("NativePath(<redacted>)")
    }
}

/// A UI-safe rendering of a native file name.
///
/// `text` may contain Unicode replacement characters when the original name
/// is not valid UTF-8. `is_lossy` lets the UI explain that fact, while all file
/// operations continue to use [`NativePath`].
#[derive(Clone, Eq, PartialEq)]
pub struct DisplayName {
    text: String,
    is_lossy: bool,
}

impl DisplayName {
    #[must_use]
    pub fn from_os_str(value: &OsStr) -> Self {
        let rendered = value.to_string_lossy();
        let is_lossy = matches!(rendered, Cow::Owned(_));
        Self {
            text: rendered.into_owned(),
            is_lossy,
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }

    #[must_use]
    pub const fn is_lossy(&self) -> bool {
        self.is_lossy
    }
}

impl fmt::Debug for DisplayName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DisplayName")
            .field("text", &"<redacted>")
            .field("is_lossy", &self.is_lossy)
            .finish()
    }
}
