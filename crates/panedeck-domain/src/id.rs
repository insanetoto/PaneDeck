use std::{fmt, num::NonZeroU64};

macro_rules! opaque_id {
    ($name:ident) => {
        #[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(NonZeroU64);

        impl $name {
            #[must_use]
            pub const fn from_raw(value: u64) -> Option<Self> {
                match NonZeroU64::new(value) {
                    Some(value) => Some(Self(value)),
                    None => None,
                }
            }

            #[must_use]
            pub const fn get(self) -> u64 {
                self.0.get()
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter
                    .debug_tuple(stringify!($name))
                    .field(&self.get())
                    .finish()
            }
        }
    };
}

opaque_id!(DirectorySessionId);
opaque_id!(EntryId);

/// Opaque reference to an open directory session.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct DirectoryReference {
    session_id: DirectorySessionId,
}

impl DirectoryReference {
    #[must_use]
    pub const fn new(session_id: DirectorySessionId) -> Self {
        Self { session_id }
    }

    #[must_use]
    pub const fn session_id(self) -> DirectorySessionId {
        self.session_id
    }
}

/// Opaque reference to an entry owned by one directory session.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct EntryReference {
    session_id: DirectorySessionId,
    entry_id: EntryId,
}

impl EntryReference {
    #[must_use]
    pub const fn new(session_id: DirectorySessionId, entry_id: EntryId) -> Self {
        Self {
            session_id,
            entry_id,
        }
    }

    #[must_use]
    pub const fn session_id(self) -> DirectorySessionId {
        self.session_id
    }

    #[must_use]
    pub const fn entry_id(self) -> EntryId {
        self.entry_id
    }
}
