use std::{
    fs::{self, File},
    io::{self, Read},
    path::Path,
};

use crate::{
    copy::{cleanup_staging, commit_staging, copy_to_staging, staging_path},
    CopyCancellation, CopyErrorCode, CopyProgressObserver, OperationKind, OperationPlan,
    OperationPlanner, PlanValidationErrorKind, PreflightProbe,
};

const VERIFY_BUFFER_SIZE: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MoveErrorCode {
    InvalidPlan,
    SourceChanged,
    Conflict,
    PermissionDenied,
    Cancelled,
    VerificationFailed,
    UnsupportedEntry,
    Io,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MoveItemOutcome {
    Moved,
    Failed(MoveErrorCode),
    CopiedButSourceNotRemoved(MoveErrorCode),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MoveItemResult {
    pub source_index: usize,
    pub outcome: MoveItemOutcome,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MoveBatchResult {
    pub items: Vec<MoveItemResult>,
}

pub trait SourceRemover {
    fn remove_source(&self, path: &Path) -> Result<(), MoveErrorCode>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct StandardSourceRemover;

impl SourceRemover for StandardSourceRemover {
    fn remove_source(&self, path: &Path) -> Result<(), MoveErrorCode> {
        let metadata = fs::symlink_metadata(path).map_err(map_io_error)?;
        if metadata.is_dir() && !metadata.file_type().is_symlink() {
            fs::remove_dir_all(path).map_err(map_io_error)
        } else {
            fs::remove_file(path).map_err(map_io_error)
        }
    }
}

trait MoveVerifier {
    fn verify(&self, source: &Path, copy: &Path) -> Result<(), MoveErrorCode>;
}

#[derive(Clone, Copy, Debug, Default)]
struct StandardMoveVerifier;

impl MoveVerifier for StandardMoveVerifier {
    fn verify(&self, source: &Path, copy: &Path) -> Result<(), MoveErrorCode> {
        verify_entry(source, copy)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct MoveExecutor;

impl MoveExecutor {
    pub fn execute<P, C, O>(
        &self,
        plan: &OperationPlan,
        probe: &P,
        cancellation: &C,
        observer: &mut O,
    ) -> MoveBatchResult
    where
        P: PreflightProbe,
        C: CopyCancellation,
        O: CopyProgressObserver,
    {
        self.execute_with(
            plan,
            probe,
            cancellation,
            observer,
            &StandardSourceRemover,
            &StandardMoveVerifier,
        )
    }

    fn execute_with<P, C, O, R, V>(
        &self,
        plan: &OperationPlan,
        probe: &P,
        cancellation: &C,
        observer: &mut O,
        remover: &R,
        verifier: &V,
    ) -> MoveBatchResult
    where
        P: PreflightProbe,
        C: CopyCancellation,
        O: CopyProgressObserver,
        R: SourceRemover,
        V: MoveVerifier,
    {
        if plan.kind() != OperationKind::Move || plan.sources().len() != plan.targets().len() {
            return failed_for_all(plan.sources().len(), MoveErrorCode::InvalidPlan);
        }
        if let Err(error) = OperationPlanner.revalidate(plan, probe) {
            return failed_for_all(plan.sources().len(), map_validation_error(error.kind()));
        }

        let total_bytes =
            plan.sources()
                .iter()
                .zip(plan.targets())
                .fold(0_u64, |total, (source, target)| {
                    if source.identity().volume_id == target.parent_identity().volume_id {
                        total
                    } else {
                        total.saturating_add(
                            probe
                                .estimated_copy_bytes(source.native_path().as_path())
                                .ok()
                                .flatten()
                                .unwrap_or_else(|| source.byte_len()),
                        )
                    }
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
            let source_path = source.native_path().as_path();
            let target_path = target.native_path().as_path();
            let outcome = if source.identity().volume_id == target.parent_identity().volume_id {
                atomic_rename_no_replace(source_path, target_path)
                    .map(|()| MoveItemOutcome::Moved)
                    .unwrap_or_else(MoveItemOutcome::Failed)
            } else {
                move_across_volumes(
                    source_path,
                    target_path,
                    cancellation,
                    observer,
                    source_index,
                    &mut bytes_copied,
                    total_bytes,
                    remover,
                    verifier,
                )
            };
            items.push(MoveItemResult {
                source_index,
                outcome,
            });
            if matches!(outcome, MoveItemOutcome::Failed(MoveErrorCode::Cancelled)) {
                append_cancelled(&mut items, source_index + 1, plan.sources().len());
                break;
            }
        }
        MoveBatchResult { items }
    }
}

#[allow(clippy::too_many_arguments)]
fn move_across_volumes<C, O, R, V>(
    source: &Path,
    target: &Path,
    cancellation: &C,
    observer: &mut O,
    source_index: usize,
    bytes_copied: &mut u64,
    total_bytes: u64,
    remover: &R,
    verifier: &V,
) -> MoveItemOutcome
where
    C: CopyCancellation,
    O: CopyProgressObserver,
    R: SourceRemover,
    V: MoveVerifier,
{
    let Some(parent) = target.parent() else {
        return MoveItemOutcome::Failed(MoveErrorCode::InvalidPlan);
    };
    let staging = staging_path(parent);
    let prepared = copy_to_staging(
        source,
        &staging,
        cancellation,
        observer,
        source_index,
        bytes_copied,
        total_bytes,
    )
    .map_err(map_copy_error)
    .and_then(|()| {
        if cancellation.is_cancelled() {
            Err(MoveErrorCode::Cancelled)
        } else {
            verifier.verify(source, &staging)
        }
    })
    .and_then(|()| commit_staging(&staging, target).map_err(map_copy_error));

    if let Err(error) = prepared {
        cleanup_staging(&staging);
        return MoveItemOutcome::Failed(error);
    }
    if let Err(error) = verifier.verify(source, target) {
        return MoveItemOutcome::CopiedButSourceNotRemoved(error);
    }
    match remover.remove_source(source) {
        Ok(()) => MoveItemOutcome::Moved,
        Err(error) => MoveItemOutcome::CopiedButSourceNotRemoved(error),
    }
}

fn verify_entry(source: &Path, copy: &Path) -> Result<(), MoveErrorCode> {
    let source_metadata = fs::symlink_metadata(source).map_err(map_io_error)?;
    let copy_metadata = fs::symlink_metadata(copy).map_err(map_io_error)?;
    let source_type = source_metadata.file_type();
    let copy_type = copy_metadata.file_type();
    if source_type.is_symlink() != copy_type.is_symlink()
        || source_metadata.is_dir() != copy_metadata.is_dir()
        || source_metadata.is_file() != copy_metadata.is_file()
    {
        return Err(MoveErrorCode::VerificationFailed);
    }
    if source_type.is_symlink() {
        return (fs::read_link(source).map_err(map_io_error)?
            == fs::read_link(copy).map_err(map_io_error)?)
        .then_some(())
        .ok_or(MoveErrorCode::VerificationFailed);
    }
    if source_metadata.is_dir() {
        let mut source_names = read_names(source)?;
        let mut copy_names = read_names(copy)?;
        source_names.sort();
        copy_names.sort();
        if source_names != copy_names {
            return Err(MoveErrorCode::VerificationFailed);
        }
        for name in source_names {
            verify_entry(&source.join(&name), &copy.join(name))?;
        }
        return Ok(());
    }
    if source_metadata.is_file() {
        if source_metadata.len() != copy_metadata.len() {
            return Err(MoveErrorCode::VerificationFailed);
        }
        return compare_files(source, copy);
    }
    Err(MoveErrorCode::UnsupportedEntry)
}

fn read_names(path: &Path) -> Result<Vec<std::ffi::OsString>, MoveErrorCode> {
    fs::read_dir(path)
        .map_err(map_io_error)?
        .map(|entry| entry.map(|value| value.file_name()).map_err(map_io_error))
        .collect()
}

fn compare_files(source: &Path, copy: &Path) -> Result<(), MoveErrorCode> {
    let mut source = File::open(source).map_err(map_io_error)?;
    let mut copy = File::open(copy).map_err(map_io_error)?;
    let mut source_buffer = vec![0_u8; VERIFY_BUFFER_SIZE];
    let mut copy_buffer = vec![0_u8; VERIFY_BUFFER_SIZE];
    loop {
        let source_read = source.read(&mut source_buffer).map_err(map_io_error)?;
        let copy_read = copy.read(&mut copy_buffer).map_err(map_io_error)?;
        if source_read != copy_read || source_buffer[..source_read] != copy_buffer[..copy_read] {
            return Err(MoveErrorCode::VerificationFailed);
        }
        if source_read == 0 {
            return Ok(());
        }
    }
}

#[cfg(target_os = "macos")]
fn atomic_rename_no_replace(source: &Path, target: &Path) -> Result<(), MoveErrorCode> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};

    let source =
        CString::new(source.as_os_str().as_bytes()).map_err(|_| MoveErrorCode::UnsupportedEntry)?;
    let target =
        CString::new(target.as_os_str().as_bytes()).map_err(|_| MoveErrorCode::UnsupportedEntry)?;
    // SAFETY: both arguments are valid, NUL-terminated path strings. RENAME_EXCL
    // preserves the plan's no-overwrite guarantee if a target appears concurrently.
    let result = unsafe { libc::renamex_np(source.as_ptr(), target.as_ptr(), libc::RENAME_EXCL) };
    if result == 0 {
        Ok(())
    } else {
        Err(map_io_error(io::Error::last_os_error()))
    }
}

#[cfg(not(target_os = "macos"))]
fn atomic_rename_no_replace(source: &Path, target: &Path) -> Result<(), MoveErrorCode> {
    if fs::symlink_metadata(target).is_ok() {
        return Err(MoveErrorCode::Conflict);
    }
    fs::rename(source, target).map_err(map_io_error)
}

fn failed_for_all(count: usize, code: MoveErrorCode) -> MoveBatchResult {
    MoveBatchResult {
        items: (0..count)
            .map(|source_index| MoveItemResult {
                source_index,
                outcome: MoveItemOutcome::Failed(code),
            })
            .collect(),
    }
}

fn append_cancelled(items: &mut Vec<MoveItemResult>, start: usize, count: usize) {
    items.extend((start..count).map(|source_index| MoveItemResult {
        source_index,
        outcome: MoveItemOutcome::Failed(MoveErrorCode::Cancelled),
    }));
}

fn map_copy_error(error: CopyErrorCode) -> MoveErrorCode {
    match error {
        CopyErrorCode::InvalidPlan => MoveErrorCode::InvalidPlan,
        CopyErrorCode::SourceChanged => MoveErrorCode::SourceChanged,
        CopyErrorCode::Conflict => MoveErrorCode::Conflict,
        CopyErrorCode::PermissionDenied => MoveErrorCode::PermissionDenied,
        CopyErrorCode::Cancelled => MoveErrorCode::Cancelled,
        CopyErrorCode::UnsupportedEntry => MoveErrorCode::UnsupportedEntry,
        CopyErrorCode::Io => MoveErrorCode::Io,
    }
}

fn map_validation_error(kind: &PlanValidationErrorKind) -> MoveErrorCode {
    match kind {
        PlanValidationErrorKind::Conflict { .. }
        | PlanValidationErrorKind::TargetChanged { .. } => MoveErrorCode::Conflict,
        PlanValidationErrorKind::MissingSource { .. }
        | PlanValidationErrorKind::SourceChanged { .. } => MoveErrorCode::SourceChanged,
        PlanValidationErrorKind::SourceNotReadable { .. }
        | PlanValidationErrorKind::ParentNotWritable
        | PlanValidationErrorKind::ParentChanged { .. } => MoveErrorCode::PermissionDenied,
        _ => MoveErrorCode::InvalidPlan,
    }
}

fn map_io_error(error: io::Error) -> MoveErrorCode {
    match error.kind() {
        io::ErrorKind::AlreadyExists => MoveErrorCode::Conflict,
        io::ErrorKind::NotFound => MoveErrorCode::SourceChanged,
        io::ErrorKind::PermissionDenied => MoveErrorCode::PermissionDenied,
        _ => MoveErrorCode::Io,
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, fs, path::PathBuf};

    use panedeck_domain::NativePath;
    use tempfile::TempDir;

    use crate::{
        AtomicCancellation, FileIdentity, NeverCancel, OperationRequest, PathPreflightState,
        ProbeError, StandardPreflightProbe,
    };

    use super::*;

    fn move_plan(source: &Path, destination: &Path) -> OperationPlan {
        OperationPlanner
            .plan(
                OperationRequest::Move {
                    sources: vec![NativePath::new(source)],
                    destination_directory: NativePath::new(destination),
                },
                &StandardPreflightProbe,
            )
            .unwrap()
    }

    #[test]
    fn same_volume_move_uses_atomic_rename() {
        let temporary = TempDir::new().unwrap();
        let source_parent = temporary.path().join("source");
        let destination = temporary.path().join("destination");
        fs::create_dir_all(&source_parent).unwrap();
        fs::create_dir_all(&destination).unwrap();
        let source = source_parent.join("file.txt");
        fs::write(&source, b"payload").unwrap();
        let plan = move_plan(&source, &destination);

        let result =
            MoveExecutor.execute(&plan, &StandardPreflightProbe, &NeverCancel, &mut |_| {});

        assert_eq!(result.items[0].outcome, MoveItemOutcome::Moved);
        assert!(!source.exists());
        assert_eq!(fs::read(destination.join("file.txt")).unwrap(), b"payload");
    }

    struct CrossVolumeProbe {
        destination: PathBuf,
    }

    impl PreflightProbe for CrossVolumeProbe {
        fn inspect(&self, path: &Path) -> Result<Option<PathPreflightState>, ProbeError> {
            let mut state = StandardPreflightProbe.inspect(path)?;
            if path == self.destination {
                if let Some(value) = &mut state {
                    let identity = value.identity.unwrap();
                    value.identity = Some(FileIdentity {
                        volume_id: identity.volume_id.wrapping_add(1),
                        file_id: identity.file_id,
                    });
                }
            }
            Ok(state)
        }

        fn estimated_copy_bytes(&self, path: &Path) -> Result<Option<u64>, ProbeError> {
            StandardPreflightProbe.estimated_copy_bytes(path)
        }

        fn available_space(&self, _directory: &Path) -> Result<Option<u64>, ProbeError> {
            Ok(Some(u64::MAX))
        }
    }

    fn cross_volume_plan(source: &Path, destination: &Path) -> (OperationPlan, CrossVolumeProbe) {
        let probe = CrossVolumeProbe {
            destination: destination.to_path_buf(),
        };
        let plan = OperationPlanner
            .plan(
                OperationRequest::Move {
                    sources: vec![NativePath::new(source)],
                    destination_directory: NativePath::new(destination),
                },
                &probe,
            )
            .unwrap();
        (plan, probe)
    }

    #[test]
    fn cross_volume_move_copies_verifies_then_removes_source() {
        let temporary = TempDir::new().unwrap();
        let source = temporary.path().join("folder");
        let destination = temporary.path().join("destination");
        fs::create_dir_all(source.join("nested")).unwrap();
        fs::create_dir_all(&destination).unwrap();
        fs::write(source.join("nested/file.txt"), b"payload").unwrap();
        let (plan, probe) = cross_volume_plan(&source, &destination);

        let result = MoveExecutor.execute(&plan, &probe, &NeverCancel, &mut |_| {});

        assert_eq!(result.items[0].outcome, MoveItemOutcome::Moved);
        assert!(!source.exists());
        assert_eq!(
            fs::read(destination.join("folder/nested/file.txt")).unwrap(),
            b"payload"
        );
    }

    struct RejectVerification;

    impl MoveVerifier for RejectVerification {
        fn verify(&self, _source: &Path, _copy: &Path) -> Result<(), MoveErrorCode> {
            Err(MoveErrorCode::VerificationFailed)
        }
    }

    #[test]
    fn verification_failure_preserves_source_and_hides_partial_target() {
        let temporary = TempDir::new().unwrap();
        let source = temporary.path().join("file.txt");
        let destination = temporary.path().join("destination");
        fs::create_dir(&destination).unwrap();
        fs::write(&source, b"payload").unwrap();
        let (plan, probe) = cross_volume_plan(&source, &destination);

        let result = MoveExecutor.execute_with(
            &plan,
            &probe,
            &NeverCancel,
            &mut |_| {},
            &StandardSourceRemover,
            &RejectVerification,
        );

        assert_eq!(
            result.items[0].outcome,
            MoveItemOutcome::Failed(MoveErrorCode::VerificationFailed)
        );
        assert!(source.exists());
        assert!(!destination.join("file.txt").exists());
    }

    struct RejectRemoval;

    impl SourceRemover for RejectRemoval {
        fn remove_source(&self, _path: &Path) -> Result<(), MoveErrorCode> {
            Err(MoveErrorCode::PermissionDenied)
        }
    }

    #[test]
    fn delete_failure_reports_copied_but_source_not_removed() {
        let temporary = TempDir::new().unwrap();
        let source = temporary.path().join("file.txt");
        let destination = temporary.path().join("destination");
        fs::create_dir(&destination).unwrap();
        fs::write(&source, b"payload").unwrap();
        let (plan, probe) = cross_volume_plan(&source, &destination);

        let result = MoveExecutor.execute_with(
            &plan,
            &probe,
            &NeverCancel,
            &mut |_| {},
            &RejectRemoval,
            &StandardMoveVerifier,
        );

        assert_eq!(
            result.items[0].outcome,
            MoveItemOutcome::CopiedButSourceNotRemoved(MoveErrorCode::PermissionDenied)
        );
        assert!(source.exists());
        assert_eq!(fs::read(destination.join("file.txt")).unwrap(), b"payload");
    }

    struct RejectAfterCommit(Cell<usize>);

    impl MoveVerifier for RejectAfterCommit {
        fn verify(&self, _source: &Path, _copy: &Path) -> Result<(), MoveErrorCode> {
            if self.0.get() == 0 {
                self.0.set(1);
                Ok(())
            } else {
                Err(MoveErrorCode::VerificationFailed)
            }
        }
    }

    #[test]
    fn source_change_after_commit_is_not_deleted() {
        let temporary = TempDir::new().unwrap();
        let source = temporary.path().join("file.txt");
        let destination = temporary.path().join("destination");
        fs::create_dir(&destination).unwrap();
        fs::write(&source, b"payload").unwrap();
        let (plan, probe) = cross_volume_plan(&source, &destination);

        let result = MoveExecutor.execute_with(
            &plan,
            &probe,
            &NeverCancel,
            &mut |_| {},
            &StandardSourceRemover,
            &RejectAfterCommit(Cell::new(0)),
        );

        assert_eq!(
            result.items[0].outcome,
            MoveItemOutcome::CopiedButSourceNotRemoved(MoveErrorCode::VerificationFailed)
        );
        assert!(source.exists());
        assert!(destination.join("file.txt").exists());
    }

    struct CancelAfterProgress<'a>(&'a AtomicCancellation, Cell<bool>);

    impl CopyProgressObserver for CancelAfterProgress<'_> {
        fn on_progress(&mut self, _progress: crate::CopyProgress) {
            if !self.1.replace(true) {
                self.0.request();
            }
        }
    }

    #[test]
    fn interrupted_copy_preserves_source_and_cleans_staging() {
        let temporary = TempDir::new().unwrap();
        let source = temporary.path().join("large.bin");
        let destination = temporary.path().join("destination");
        fs::create_dir(&destination).unwrap();
        fs::write(&source, vec![7_u8; VERIFY_BUFFER_SIZE * 2]).unwrap();
        let (plan, probe) = cross_volume_plan(&source, &destination);
        let cancellation = AtomicCancellation::default();
        let mut observer = CancelAfterProgress(&cancellation, Cell::new(false));

        let result = MoveExecutor.execute(&plan, &probe, &cancellation, &mut observer);

        assert_eq!(
            result.items[0].outcome,
            MoveItemOutcome::Failed(MoveErrorCode::Cancelled)
        );
        assert!(source.exists());
        assert!(!destination.join("large.bin").exists());
        assert!(fs::read_dir(&destination).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("partial")));
    }

    #[test]
    fn planner_rejects_moving_a_directory_into_itself() {
        let temporary = TempDir::new().unwrap();
        let source = temporary.path().join("folder");
        let child = source.join("child");
        fs::create_dir_all(&child).unwrap();
        let error = OperationPlanner
            .plan(
                OperationRequest::Move {
                    sources: vec![NativePath::new(&source)],
                    destination_directory: NativePath::new(&child),
                },
                &StandardPreflightProbe,
            )
            .unwrap_err();
        assert!(matches!(
            error.kind(),
            PlanValidationErrorKind::DirectoryIntoItself { source_index: 0 }
        ));
    }
}
