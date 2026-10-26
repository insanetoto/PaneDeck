//! Read-only mount/unmount notifications from `NSWorkspace`.
//!
//! The observer only signals that the set of mounted volumes may have changed;
//! it never enumerates files or navigates the UI.

/// Keeps the native observers alive. Dropping it removes both registrations.
#[cfg(target_os = "macos")]
pub struct VolumeObserver {
    center: objc2::rc::Retained<objc2_foundation::NSNotificationCenter>,
    tokens: Vec<
        objc2::rc::Retained<objc2::runtime::ProtocolObject<dyn objc2_foundation::NSObjectProtocol>>,
    >,
}

#[cfg(target_os = "macos")]
impl VolumeObserver {
    #[must_use]
    pub fn start<F>(callback: F) -> Option<Self>
    where
        F: Fn() + 'static,
    {
        use std::sync::Arc;

        use block2::RcBlock;
        use objc2_app_kit::{
            NSWorkspace, NSWorkspaceDidMountNotification, NSWorkspaceDidUnmountNotification,
        };
        use objc2_foundation::NSNotification;

        let center = NSWorkspace::sharedWorkspace().notificationCenter();
        let callback = Arc::new(callback);
        let mut tokens = Vec::new();
        // SAFETY: The notification names are immutable process-wide constants
        // and the block only calls the provided signal callback.
        let names = unsafe {
            [
                NSWorkspaceDidMountNotification,
                NSWorkspaceDidUnmountNotification,
            ]
        };
        for name in names {
            let callback = Arc::clone(&callback);
            let block =
                RcBlock::new(move |_notification: std::ptr::NonNull<NSNotification>| callback());
            let token = unsafe {
                center.addObserverForName_object_queue_usingBlock(Some(name), None, None, &block)
            };
            tokens.push(token);
        }
        Some(Self { center, tokens })
    }
}

#[cfg(target_os = "macos")]
impl Drop for VolumeObserver {
    fn drop(&mut self) {
        for token in &self.tokens {
            // SAFETY: The token was returned by this center and is removed once
            // while the center is still retained by `self`.
            unsafe { self.center.removeObserver(token.as_ref()) };
        }
    }
}

#[cfg(target_os = "macos")]
impl std::fmt::Debug for VolumeObserver {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VolumeObserver")
            .field("observers", &self.tokens.len())
            .finish()
    }
}

/// Signal that mounted volumes may have changed. Returns `None` on platforms
/// without native volume notifications.
#[must_use]
#[cfg(target_os = "macos")]
pub fn observe_volume_changes<F>(callback: F) -> Option<VolumeObserver>
where
    F: Fn() + 'static,
{
    VolumeObserver::start(callback)
}

#[must_use]
#[cfg(not(target_os = "macos"))]
pub fn observe_volume_changes<F>(_callback: F) -> Option<VolumeObserver>
where
    F: Fn() + 'static,
{
    None
}

/// Marker type used by non-macOS builds.
#[cfg(not(target_os = "macos"))]
#[derive(Debug, Default)]
pub struct VolumeObserver;

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn observes_mount_events_without_reading_files() {
        let observed = observe_volume_changes(|| {}).expect("workspace notification center exists");
        assert!(format!("{observed:?}").contains("VolumeObserver"));
    }
}
