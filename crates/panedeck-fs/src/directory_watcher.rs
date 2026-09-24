use std::{
    error::Error,
    fmt, io,
    path::Path,
    sync::mpsc::{self, RecvTimeoutError},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use notify::{RecursiveMode, Watcher};

const DEBOUNCE_WINDOW: Duration = Duration::from_millis(150);
const STOP_POLL_INTERVAL: Duration = Duration::from_millis(25);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WatchSignal {
    /// Something may have changed. Consumers must perform a complete rescan.
    RescanRequired,
    /// The native watcher reported an error. A complete rescan is still required.
    WatchFailed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WatchError {
    NotFound,
    PermissionDenied,
    NotDirectory,
    Io,
}

impl fmt::Display for WatchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NotFound => "watch root was not found",
            Self::PermissionDenied => "watch root is not accessible",
            Self::NotDirectory => "watch root is not a directory",
            Self::Io => "directory watcher could not be started",
        })
    }
}

impl Error for WatchError {}

#[derive(Clone, Copy, Debug, Default)]
pub struct DirectoryWatcher;

impl DirectoryWatcher {
    pub fn start<F>(&self, path: &Path, on_signal: F) -> Result<DirectoryWatch, WatchError>
    where
        F: Fn(WatchSignal) + Send + 'static,
    {
        validate_root(path)?;
        let (native_sender, native_receiver) = mpsc::channel();
        let mut watcher = create_platform_watcher(move |event| {
            let _ = native_sender.send(event);
        })?;
        watcher
            .watch(path, RecursiveMode::NonRecursive)
            .map_err(|_| WatchError::Io)?;

        let (stop_sender, stop_receiver) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("panedeck-directory-watch".into())
            .spawn(move || {
                let watcher = watcher;
                let mut pending = None;
                loop {
                    let _keep_watcher_alive = &watcher;
                    if stop_receiver.try_recv().is_ok() {
                        break;
                    }
                    match native_receiver.recv_timeout(STOP_POLL_INTERVAL) {
                        Ok(Ok(_)) => pending = merge_pending(pending, WatchSignal::RescanRequired),
                        Ok(Err(_)) => pending = merge_pending(pending, WatchSignal::WatchFailed),
                        Err(RecvTimeoutError::Disconnected) => {
                            on_signal(WatchSignal::WatchFailed);
                            break;
                        }
                        Err(RecvTimeoutError::Timeout) => {}
                    }
                    if let Some((signal, deadline)) = pending {
                        if Instant::now() >= deadline {
                            on_signal(signal);
                            pending = None;
                        }
                    }
                }
            })
            .map_err(|_| WatchError::Io)?;

        Ok(DirectoryWatch {
            stop_sender: Some(stop_sender),
            worker: Some(worker),
        })
    }
}

#[cfg(not(test))]
type PlatformWatcher = notify::RecommendedWatcher;

#[cfg(test)]
type PlatformWatcher = notify::PollWatcher;

#[cfg(not(test))]
fn create_platform_watcher<F>(handler: F) -> Result<PlatformWatcher, WatchError>
where
    F: notify::EventHandler,
{
    notify::recommended_watcher(handler).map_err(|_| WatchError::Io)
}

#[cfg(test)]
fn create_platform_watcher<F>(handler: F) -> Result<PlatformWatcher, WatchError>
where
    F: notify::EventHandler,
{
    notify::PollWatcher::new(
        handler,
        notify::Config::default().with_poll_interval(Duration::from_millis(50)),
    )
    .map_err(|_| WatchError::Io)
}

pub struct DirectoryWatch {
    stop_sender: Option<mpsc::Sender<()>>,
    worker: Option<JoinHandle<()>>,
}

impl DirectoryWatch {
    pub fn stop(mut self) {
        self.stop_and_join();
    }

    fn stop_and_join(&mut self) {
        if let Some(sender) = self.stop_sender.take() {
            let _ = sender.send(());
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for DirectoryWatch {
    fn drop(&mut self) {
        self.stop_and_join();
    }
}

fn merge_pending(
    current: Option<(WatchSignal, Instant)>,
    next: WatchSignal,
) -> Option<(WatchSignal, Instant)> {
    let signal = if matches!(next, WatchSignal::WatchFailed)
        || matches!(current, Some((WatchSignal::WatchFailed, _)))
    {
        WatchSignal::WatchFailed
    } else {
        WatchSignal::RescanRequired
    };
    Some((signal, Instant::now() + DEBOUNCE_WINDOW))
}

fn validate_root(path: &Path) -> Result<(), WatchError> {
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) => Err(WatchError::NotDirectory),
        Err(error) => Err(map_io_error(error)),
    }
}

fn map_io_error(error: io::Error) -> WatchError {
    match error.kind() {
        io::ErrorKind::NotFound => WatchError::NotFound,
        io::ErrorKind::PermissionDenied => WatchError::PermissionDenied,
        io::ErrorKind::NotADirectory => WatchError::NotDirectory,
        _ => WatchError::Io,
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, sync::mpsc, time::Duration};

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn rejects_missing_and_non_directory_roots() {
        let temporary = TempDir::new().unwrap();
        let file = temporary.path().join("file.txt");
        fs::write(&file, b"content").unwrap();

        assert!(matches!(
            DirectoryWatcher.start(&file, |_| {}),
            Err(WatchError::NotDirectory)
        ));
        assert!(matches!(
            DirectoryWatcher.start(&temporary.path().join("missing"), |_| {}),
            Err(WatchError::NotFound)
        ));
    }

    #[test]
    fn create_rename_delete_and_event_storm_request_a_rescan() {
        let temporary = TempDir::new().unwrap();
        let (sender, receiver) = mpsc::channel();
        let watch = DirectoryWatcher
            .start(temporary.path(), move |signal| {
                let _ = sender.send(signal);
            })
            .unwrap();

        let ready_deadline = Instant::now() + Duration::from_secs(5);
        let mut ready_index = 0;
        loop {
            fs::write(
                temporary.path().join(format!("ready-{ready_index}.txt")),
                b"ready",
            )
            .unwrap();
            ready_index += 1;
            if receiver.recv_timeout(Duration::from_millis(250)).is_ok() {
                break;
            }
            assert!(
                Instant::now() < ready_deadline,
                "watcher did not become ready"
            );
        }

        let first = temporary.path().join("first.txt");
        let renamed = temporary.path().join("renamed.txt");
        fs::write(&first, b"one").unwrap();
        fs::rename(&first, &renamed).unwrap();
        for index in 0..32 {
            fs::write(temporary.path().join(format!("batch-{index}.txt")), b"x").unwrap();
        }
        fs::remove_file(renamed).unwrap();

        assert_eq!(
            receiver.recv_timeout(Duration::from_secs(5)).unwrap(),
            WatchSignal::RescanRequired
        );
        watch.stop();
    }

    #[test]
    fn dropping_a_watch_stops_its_worker() {
        let temporary = TempDir::new().unwrap();
        let (sender, receiver) = mpsc::channel();
        let watch = DirectoryWatcher
            .start(temporary.path(), move |signal| {
                let _ = sender.send(signal);
            })
            .unwrap();
        watch.stop();

        fs::write(temporary.path().join("after-stop.txt"), b"x").unwrap();
        assert!(receiver.recv_timeout(Duration::from_millis(300)).is_err());
    }
}
