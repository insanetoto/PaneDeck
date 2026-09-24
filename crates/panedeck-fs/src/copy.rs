use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
};

use filetime::FileTime;

use crate::{
    OperationKind, OperationPlan, OperationPlanner, PlanValidationErrorKind, PreflightProbe,
};

const COPY_BUFFER_SIZE: usize = 1024 * 1024;
static NEXT_STAGING_ID: AtomicU64 = AtomicU64::new(1);

pub trait CopyCancellation {
    fn is_cancelled(&self) -> bool;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NeverCancel;

impl CopyCancellation for NeverCancel {
    fn is_cancelled(&self) -> bool {
        false
    }
}

#[derive(Debug, Default)]
pub struct AtomicCancellation(AtomicBool);

impl AtomicCancellation {
    pub fn request(&self) {
        self.0.store(true, Ordering::Release);
    }
}

impl CopyCancellation for AtomicCancellation {
    fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

impl<T: CopyCancellation> CopyCancellation for Arc<T> {
    fn is_cancelled(&self) -> bool {
        self.as_ref().is_cancelled()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CopyProgress {
    pub source_index: usize,
    pub bytes_copied: u64,
    pub total_bytes: u64,
}

pub trait CopyProgressObserver {
    fn on_progress(&mut self, progress: CopyProgress);
}

impl<F> CopyProgressObserver for F
where
    F: FnMut(CopyProgress),
{
    fn on_progress(&mut self, progress: CopyProgress) {
        self(progress);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CopyErrorCode {
    InvalidPlan,
    SourceChanged,
    Conflict,
    PermissionDenied,
    Cancelled,
    UnsupportedEntry,
    Io,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CopyItemOutcome {
    Copied,
    Failed(CopyErrorCode),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CopyItemResult {
    pub source_index: usize,
    pub outcome: CopyItemOutcome,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CopyBatchResult {
    pub items: Vec<CopyItemResult>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CopyExecutor;

impl CopyExecutor {
    pub fn execute<P, C, O>(
        &self,
        plan: &OperationPlan,
        probe: &P,
        cancellation: &C,
        observer: &mut O,
    ) -> CopyBatchResult
    where
        P: PreflightProbe,
        C: CopyCancellation,
        O: CopyProgressObserver,
    {
        if plan.kind() != OperationKind::Copy || plan.sources().len() != plan.targets().len() {
            return failed_for_all(plan.sources().len(), CopyErrorCode::InvalidPlan);
        }
        if let Err(error) = OperationPlanner.revalidate(plan, probe) {
            return failed_for_all(plan.sources().len(), map_validation_error(error.kind()));
        }

        let total_bytes = plan.sources().iter().fold(0_u64, |total, source| {
            total.saturating_add(
                probe
                    .estimated_copy_bytes(source.native_path().as_path())
                    .ok()
                    .flatten()
                    .unwrap_or_else(|| source.byte_len()),
            )
        });
        let mut bytes_copied = 0_u64;
        let mut items = Vec::with_capacity(plan.sources().len());

        for (source_index, (source, target)) in
            plan.sources().iter().zip(plan.targets()).enumerate()
        {
            if cancellation.is_cancelled() {
                append_cancelled(&mut items, source_index, plan.sources().len());
                break;
            }
            let target_path = target.native_path().as_path();
            let Some(parent) = target_path.parent() else {
                items.push(CopyItemResult {
                    source_index,
                    outcome: CopyItemOutcome::Failed(CopyErrorCode::InvalidPlan),
                });
                continue;
            };
            let staging = staging_path(parent);
            let result = copy_to_staging(
                source.native_path().as_path(),
                &staging,
                cancellation,
                observer,
                source_index,
                &mut bytes_copied,
                total_bytes,
            )
            .and_then(|()| commit_staging(&staging, target_path));

            match result {
                Ok(()) => items.push(CopyItemResult {
                    source_index,
                    outcome: CopyItemOutcome::Copied,
                }),
                Err(code) => {
                    cleanup_staging(&staging);
                    items.push(CopyItemResult {
                        source_index,
                        outcome: CopyItemOutcome::Failed(code),
                    });
                    if code == CopyErrorCode::Cancelled {
                        append_cancelled(&mut items, source_index + 1, plan.sources().len());
                        break;
                    }
                }
            }
        }

        CopyBatchResult { items }
    }
}

fn failed_for_all(count: usize, code: CopyErrorCode) -> CopyBatchResult {
    CopyBatchResult {
        items: (0..count)
            .map(|source_index| CopyItemResult {
                source_index,
                outcome: CopyItemOutcome::Failed(code),
            })
            .collect(),
    }
}

fn append_cancelled(items: &mut Vec<CopyItemResult>, start: usize, count: usize) {
    items.extend((start..count).map(|source_index| CopyItemResult {
        source_index,
        outcome: CopyItemOutcome::Failed(CopyErrorCode::Cancelled),
    }));
}

pub(crate) fn staging_path(parent: &Path) -> PathBuf {
    let id = NEXT_STAGING_ID.fetch_add(1, Ordering::Relaxed);
    parent.join(format!(
        ".panedeck-copy-{}-{id}.partial",
        std::process::id()
    ))
}

pub(crate) fn copy_to_staging<C, O>(
    source: &Path,
    target: &Path,
    cancellation: &C,
    observer: &mut O,
    source_index: usize,
    bytes_copied: &mut u64,
    total_bytes: u64,
) -> Result<(), CopyErrorCode>
where
    C: CopyCancellation,
    O: CopyProgressObserver,
{
    if cancellation.is_cancelled() {
        return Err(CopyErrorCode::Cancelled);
    }
    let metadata = fs::symlink_metadata(source).map_err(map_io_error)?;
    if metadata.file_type().is_symlink() {
        copy_symlink(source, target)?;
    } else if metadata.is_dir() {
        fs::create_dir(target).map_err(map_io_error)?;
        let children = fs::read_dir(source).map_err(map_io_error)?;
        for child in children {
            let child = child.map_err(map_io_error)?;
            copy_to_staging(
                &child.path(),
                &target.join(child.file_name()),
                cancellation,
                observer,
                source_index,
                bytes_copied,
                total_bytes,
            )?;
        }
        apply_metadata(target, &metadata)?;
    } else if metadata.is_file() {
        copy_file(
            source,
            target,
            cancellation,
            observer,
            source_index,
            bytes_copied,
            total_bytes,
        )?;
        apply_metadata(target, &metadata)?;
    } else {
        return Err(CopyErrorCode::UnsupportedEntry);
    }
    Ok(())
}

fn copy_file<C, O>(
    source: &Path,
    target: &Path,
    cancellation: &C,
    observer: &mut O,
    source_index: usize,
    bytes_copied: &mut u64,
    total_bytes: u64,
) -> Result<(), CopyErrorCode>
where
    C: CopyCancellation,
    O: CopyProgressObserver,
{
    let mut input = File::open(source).map_err(map_io_error)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)
        .map_err(map_io_error)?;
    let mut buffer = vec![0_u8; COPY_BUFFER_SIZE];
    loop {
        if cancellation.is_cancelled() {
            return Err(CopyErrorCode::Cancelled);
        }
        let read = input.read(&mut buffer).map_err(map_io_error)?;
        if read == 0 {
            break;
        }
        output.write_all(&buffer[..read]).map_err(map_io_error)?;
        *bytes_copied = bytes_copied.saturating_add(read as u64);
        observer.on_progress(CopyProgress {
            source_index,
            bytes_copied: *bytes_copied,
            total_bytes,
        });
    }
    output.sync_all().map_err(map_io_error)
}

fn apply_metadata(target: &Path, metadata: &fs::Metadata) -> Result<(), CopyErrorCode> {
    fs::set_permissions(target, metadata.permissions()).map_err(map_io_error)?;
    let accessed = FileTime::from_last_access_time(metadata);
    let modified = FileTime::from_last_modification_time(metadata);
    filetime::set_file_times(target, accessed, modified).map_err(map_io_error)
}

#[cfg(unix)]
fn copy_symlink(source: &Path, target: &Path) -> Result<(), CopyErrorCode> {
    std::os::unix::fs::symlink(fs::read_link(source).map_err(map_io_error)?, target)
        .map_err(map_io_error)
}

#[cfg(not(unix))]
fn copy_symlink(_source: &Path, _target: &Path) -> Result<(), CopyErrorCode> {
    Err(CopyErrorCode::UnsupportedEntry)
}

pub(crate) fn commit_staging(staging: &Path, target: &Path) -> Result<(), CopyErrorCode> {
    if fs::symlink_metadata(target).is_ok() {
        return Err(CopyErrorCode::Conflict);
    }
    if fs::symlink_metadata(staging)
        .map_err(map_io_error)?
        .is_dir()
    {
        rename_directory_no_replace(staging, target)
    } else {
        fs::hard_link(staging, target).map_err(map_io_error)?;
        fs::remove_file(staging).map_err(map_io_error)
    }
}

#[cfg(target_os = "macos")]
fn rename_directory_no_replace(source: &Path, target: &Path) -> Result<(), CopyErrorCode> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};

    let source =
        CString::new(source.as_os_str().as_bytes()).map_err(|_| CopyErrorCode::UnsupportedEntry)?;
    let target =
        CString::new(target.as_os_str().as_bytes()).map_err(|_| CopyErrorCode::UnsupportedEntry)?;

    // SAFETY: both arguments are valid, NUL-terminated path strings and RENAME_EXCL
    // asks macOS to fail instead of replacing a target created after validation.
    let result = unsafe { libc::renamex_np(source.as_ptr(), target.as_ptr(), libc::RENAME_EXCL) };
    if result == 0 {
        Ok(())
    } else {
        Err(map_io_error(io::Error::last_os_error()))
    }
}

#[cfg(not(target_os = "macos"))]
fn rename_directory_no_replace(source: &Path, target: &Path) -> Result<(), CopyErrorCode> {
    if fs::symlink_metadata(target).is_ok() {
        return Err(CopyErrorCode::Conflict);
    }
    fs::rename(source, target).map_err(map_io_error)
}

pub(crate) fn cleanup_staging(path: &Path) {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return;
    };
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        let _ = fs::remove_dir_all(path);
    } else {
        let _ = fs::remove_file(path);
    }
}

fn map_validation_error(kind: &PlanValidationErrorKind) -> CopyErrorCode {
    match kind {
        PlanValidationErrorKind::Conflict { .. }
        | PlanValidationErrorKind::TargetChanged { .. } => CopyErrorCode::Conflict,
        PlanValidationErrorKind::SourceChanged { .. }
        | PlanValidationErrorKind::MissingSource { .. } => CopyErrorCode::SourceChanged,
        PlanValidationErrorKind::SourceNotReadable { .. }
        | PlanValidationErrorKind::ParentNotWritable => CopyErrorCode::PermissionDenied,
        _ => CopyErrorCode::InvalidPlan,
    }
}

fn map_io_error(error: io::Error) -> CopyErrorCode {
    match error.kind() {
        io::ErrorKind::AlreadyExists => CopyErrorCode::Conflict,
        io::ErrorKind::NotFound => CopyErrorCode::SourceChanged,
        io::ErrorKind::PermissionDenied => CopyErrorCode::PermissionDenied,
        _ => CopyErrorCode::Io,
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, io::Write};

    use panedeck_domain::{EntryKind, NativePath};
    use tempfile::TempDir;

    use crate::{
        FileIdentity, OperationRequest, PathPreflightState, ProbeError, StandardPreflightProbe,
    };

    use super::*;

    fn copy_plan(source: &Path, destination: &Path) -> OperationPlan {
        OperationPlanner
            .plan(
                OperationRequest::Copy {
                    sources: vec![NativePath::new(source)],
                    destination_directory: NativePath::new(destination),
                },
                &StandardPreflightProbe,
            )
            .expect("copy plan should be valid")
    }

    fn execute(plan: &OperationPlan) -> CopyBatchResult {
        CopyExecutor.execute(plan, &StandardPreflightProbe, &NeverCancel, &mut |_| {})
    }

    #[test]
    fn copies_regular_empty_and_large_files_with_metadata() {
        let temp = TempDir::new().expect("temp dir");
        let destination = temp.path().join("destination");
        fs::create_dir(&destination).expect("destination");
        for (name, bytes) in [
            ("regular.bin", vec![7_u8; 1024]),
            ("empty.bin", Vec::new()),
            ("large.bin", vec![9_u8; COPY_BUFFER_SIZE * 2 + 13]),
        ] {
            let source = temp.path().join(name);
            fs::write(&source, &bytes).expect("source");
            let mut permissions = fs::metadata(&source).expect("metadata").permissions();
            permissions.set_readonly(true);
            fs::set_permissions(&source, permissions).expect("permissions");
            let plan = copy_plan(&source, &destination);
            assert_eq!(execute(&plan).items[0].outcome, CopyItemOutcome::Copied);
            assert_eq!(
                fs::read(destination.join(name)).expect("copied file"),
                bytes
            );
            assert!(fs::metadata(destination.join(name))
                .expect("target metadata")
                .permissions()
                .readonly());
        }
    }

    #[cfg(unix)]
    #[test]
    fn copies_nested_directories_and_preserves_symlinks() {
        let temp = TempDir::new().expect("temp dir");
        let source = temp.path().join("tree");
        let destination = temp.path().join("destination");
        fs::create_dir_all(source.join("nested")).expect("tree");
        fs::create_dir(&destination).expect("destination");
        fs::write(source.join("nested/data.txt"), b"payload").expect("file");
        std::os::unix::fs::symlink("nested/data.txt", source.join("link")).expect("symlink");

        assert_eq!(
            execute(&copy_plan(&source, &destination)).items[0].outcome,
            CopyItemOutcome::Copied
        );
        assert_eq!(
            fs::read(destination.join("tree/nested/data.txt")).expect("copied file"),
            b"payload"
        );
        assert_eq!(
            fs::read_link(destination.join("tree/link")).expect("copied symlink"),
            PathBuf::from("nested/data.txt")
        );
    }

    #[test]
    fn cancellation_removes_staging_and_never_publishes_partial_target() {
        let temp = TempDir::new().expect("temp dir");
        let source = temp.path().join("large.bin");
        let destination = temp.path().join("destination");
        fs::create_dir(&destination).expect("destination");
        let mut file = File::create(&source).expect("source");
        file.write_all(&vec![5_u8; COPY_BUFFER_SIZE * 3])
            .expect("source bytes");
        drop(file);
        let plan = copy_plan(&source, &destination);
        let cancellation = Arc::new(AtomicCancellation::default());
        let signal = Arc::clone(&cancellation);
        let result = CopyExecutor.execute(
            &plan,
            &StandardPreflightProbe,
            &cancellation,
            &mut move |_| signal.request(),
        );

        assert_eq!(
            result.items[0].outcome,
            CopyItemOutcome::Failed(CopyErrorCode::Cancelled)
        );
        assert!(!destination.join("large.bin").exists());
        assert!(fs::read_dir(&destination)
            .expect("destination")
            .next()
            .is_none());
    }

    #[test]
    fn target_appearing_after_plan_is_a_conflict_and_is_not_overwritten() {
        let temp = TempDir::new().expect("temp dir");
        let source = temp.path().join("data.txt");
        let destination = temp.path().join("destination");
        fs::create_dir(&destination).expect("destination");
        fs::write(&source, b"source").expect("source");
        let plan = copy_plan(&source, &destination);
        fs::write(destination.join("data.txt"), b"existing").expect("conflict");

        assert_eq!(
            execute(&plan).items[0].outcome,
            CopyItemOutcome::Failed(CopyErrorCode::Conflict)
        );
        assert_eq!(
            fs::read(destination.join("data.txt")).expect("existing target"),
            b"existing"
        );
    }

    #[cfg(unix)]
    #[test]
    fn permission_failure_is_reported_without_creating_a_target() {
        use std::os::unix::fs::PermissionsExt;

        let temp = TempDir::new().expect("temp dir");
        let source = temp.path().join("private.txt");
        let destination = temp.path().join("destination");
        fs::create_dir(&destination).expect("destination");
        fs::write(&source, b"private").expect("source");
        let mut permissions = fs::metadata(&source).expect("metadata").permissions();
        permissions.set_mode(0o000);
        fs::set_permissions(&source, permissions).expect("restrict");
        let result = OperationPlanner.plan(
            OperationRequest::Copy {
                sources: vec![NativePath::new(&source)],
                destination_directory: NativePath::new(&destination),
            },
            &StandardPreflightProbe,
        );
        let mut restore = fs::metadata(&source).expect("metadata").permissions();
        restore.set_mode(0o600);
        fs::set_permissions(&source, restore).expect("restore");

        assert!(matches!(
            result.expect_err("unreadable source must fail").kind(),
            PlanValidationErrorKind::SourceNotReadable { .. }
        ));
        assert!(!destination.join("private.txt").exists());
    }

    struct NoSpaceProbe {
        states: HashMap<PathBuf, PathPreflightState>,
    }

    impl PreflightProbe for NoSpaceProbe {
        fn inspect(&self, path: &Path) -> Result<Option<PathPreflightState>, ProbeError> {
            Ok(self.states.get(path).copied())
        }

        fn estimated_copy_bytes(&self, _path: &Path) -> Result<Option<u64>, ProbeError> {
            Ok(Some(10))
        }

        fn available_space(&self, _directory: &Path) -> Result<Option<u64>, ProbeError> {
            Ok(Some(1))
        }
    }

    #[test]
    fn insufficient_space_is_rejected_before_copying() {
        let source = PathBuf::from("/source/file");
        let destination = PathBuf::from("/destination");
        let probe = NoSpaceProbe {
            states: HashMap::from([
                (
                    source.clone(),
                    PathPreflightState {
                        identity: Some(FileIdentity {
                            volume_id: 1,
                            file_id: 1,
                        }),
                        kind: EntryKind::File,
                        byte_len: 10,
                        is_readable: true,
                        is_writable: true,
                    },
                ),
                (
                    destination.clone(),
                    PathPreflightState {
                        identity: Some(FileIdentity {
                            volume_id: 2,
                            file_id: 2,
                        }),
                        kind: EntryKind::Directory,
                        byte_len: 0,
                        is_readable: true,
                        is_writable: true,
                    },
                ),
            ]),
        };
        let result = OperationPlanner.plan(
            OperationRequest::Copy {
                sources: vec![NativePath::new(source)],
                destination_directory: NativePath::new(destination),
            },
            &probe,
        );
        assert!(matches!(
            result.expect_err("space check must fail").kind(),
            PlanValidationErrorKind::InsufficientSpace { .. }
        ));
    }
}
