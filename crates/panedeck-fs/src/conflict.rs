use std::{error::Error, ffi::OsString, fmt, path::Path};

use panedeck_domain::{EntryKind, NativePath};

use crate::{FileIdentity, OperationKind, PathPreflightState, PreflightProbe, ProbeError};

const MAX_KEEP_BOTH_ATTEMPTS: u32 = 10_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConflictDecision {
    Skip,
    Replace,
    KeepBoth,
    Cancel,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConflictScope {
    ThisConflict,
    RemainingConflicts,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConflictKind {
    FileReplacement,
    DirectoryMerge,
    TypeMismatch,
}

#[derive(Clone, Debug)]
pub struct Conflict {
    item_index: usize,
    source: NativePath,
    source_identity: FileIdentity,
    source_kind: EntryKind,
    target: NativePath,
    target_identity: FileIdentity,
    target_kind: EntryKind,
    kind: ConflictKind,
}

impl Conflict {
    #[must_use]
    pub const fn item_index(&self) -> usize {
        self.item_index
    }

    #[must_use]
    pub const fn kind(&self) -> ConflictKind {
        self.kind
    }

    #[must_use]
    pub const fn source_kind(&self) -> EntryKind {
        self.source_kind
    }

    #[must_use]
    pub const fn source(&self) -> &NativePath {
        &self.source
    }

    #[must_use]
    pub const fn target_kind(&self) -> EntryKind {
        self.target_kind
    }

    #[must_use]
    pub const fn target_identity(&self) -> FileIdentity {
        self.target_identity
    }

    #[must_use]
    pub const fn target(&self) -> &NativePath {
        &self.target
    }
}

#[derive(Clone, Debug)]
pub struct ConflictGuard {
    item_index: usize,
    source: NativePath,
    source_identity: FileIdentity,
    source_kind: EntryKind,
    target: NativePath,
    expected_target: Option<(FileIdentity, EntryKind)>,
}

#[derive(Clone, Debug)]
pub enum ConflictAction {
    ReplaceFile(ConflictGuard),
    MergeDirectory(ConflictGuard),
    KeepBoth(ConflictGuard),
}

impl ConflictAction {
    #[must_use]
    pub const fn target(&self) -> &NativePath {
        match self {
            Self::ReplaceFile(guard) | Self::MergeDirectory(guard) | Self::KeepBoth(guard) => {
                &guard.target
            }
        }
    }

    #[must_use]
    pub const fn item_index(&self) -> usize {
        match self {
            Self::ReplaceFile(guard) | Self::MergeDirectory(guard) | Self::KeepBoth(guard) => {
                guard.item_index
            }
        }
    }
}

#[derive(Clone, Debug)]
pub enum ConflictResolution {
    NoConflict { target: NativePath },
    AwaitingDecision(Conflict),
    Ready(ConflictAction),
    Skipped { item_index: usize },
    Cancelled,
}

#[derive(Clone, Debug)]
pub enum ActionReadiness {
    Ready,
    TargetVacated,
    AwaitingDecision(Conflict),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConflictError {
    UnsupportedOperation,
    MissingSource,
    SourceChanged,
    SameEntry,
    IdentityUnavailable,
    ReplaceTypeMismatch,
    KeepBothNameExhausted,
    TaskCancelled,
    ProbeFailed(ProbeError),
}

impl fmt::Display for ConflictError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "conflict resolution failed: {self:?}")
    }
}

impl Error for ConflictError {}

impl From<ProbeError> for ConflictError {
    fn from(value: ProbeError) -> Self {
        Self::ProbeFailed(value)
    }
}

/// Task-local conflict state. A policy selected for remaining conflicts never
/// escapes this instance and therefore cannot affect another job.
#[derive(Clone, Debug)]
pub struct ConflictSession {
    operation: OperationKind,
    remaining_policy: Option<ConflictDecision>,
    cancelled: bool,
}

impl ConflictSession {
    pub fn new(operation: OperationKind) -> Result<Self, ConflictError> {
        if !matches!(
            operation,
            OperationKind::Copy | OperationKind::Move | OperationKind::Rename
        ) {
            return Err(ConflictError::UnsupportedOperation);
        }
        Ok(Self {
            operation,
            remaining_policy: None,
            cancelled: false,
        })
    }

    #[must_use]
    pub const fn operation(&self) -> OperationKind {
        self.operation
    }

    pub fn encounter<P: PreflightProbe>(
        &mut self,
        item_index: usize,
        source: NativePath,
        target: NativePath,
        probe: &P,
    ) -> Result<ConflictResolution, ConflictError> {
        if self.cancelled {
            return Err(ConflictError::TaskCancelled);
        }
        let source_state = probe
            .inspect(source.as_path())?
            .ok_or(ConflictError::MissingSource)?;
        let source_identity = identity(source_state)?;
        let Some(target_state) = probe.inspect(target.as_path())? else {
            return Ok(ConflictResolution::NoConflict { target });
        };
        let conflict = make_conflict(
            item_index,
            source,
            source_identity,
            source_state.kind,
            target,
            target_state,
        )?;
        if let Some(decision) = self.remaining_policy {
            self.resolve_current(conflict, decision, probe)
        } else {
            Ok(ConflictResolution::AwaitingDecision(conflict))
        }
    }

    pub fn decide<P: PreflightProbe>(
        &mut self,
        conflict: Conflict,
        decision: ConflictDecision,
        scope: ConflictScope,
        probe: &P,
    ) -> Result<ConflictResolution, ConflictError> {
        if self.cancelled {
            return Err(ConflictError::TaskCancelled);
        }
        match refresh_conflict(&conflict, probe)? {
            ConflictRefresh::Unchanged => {}
            ConflictRefresh::Vacated => {
                return Ok(ConflictResolution::NoConflict {
                    target: conflict.target,
                });
            }
            ConflictRefresh::Changed(changed) => {
                return Ok(ConflictResolution::AwaitingDecision(changed));
            }
        }
        let resolution = self.resolve_current(conflict, decision, probe)?;
        if scope == ConflictScope::RemainingConflicts
            && !matches!(decision, ConflictDecision::Cancel)
        {
            self.remaining_policy = Some(decision);
        }
        Ok(resolution)
    }

    pub fn revalidate_action<P: PreflightProbe>(
        &self,
        action: &ConflictAction,
        probe: &P,
    ) -> Result<ActionReadiness, ConflictError> {
        let guard = action.guard();
        let source_state = probe
            .inspect(guard.source.as_path())?
            .ok_or(ConflictError::SourceChanged)?;
        if source_state.identity != Some(guard.source_identity)
            || source_state.kind != guard.source_kind
        {
            return Err(ConflictError::SourceChanged);
        }
        let current = probe.inspect(guard.target.as_path())?;
        match (guard.expected_target, current) {
            (None, None) => Ok(ActionReadiness::Ready),
            (Some(_), None) => Ok(ActionReadiness::TargetVacated),
            (expected, Some(state))
                if expected == state.identity.map(|identity| (identity, state.kind)) =>
            {
                Ok(ActionReadiness::Ready)
            }
            (_, Some(state)) => Ok(ActionReadiness::AwaitingDecision(make_conflict(
                guard.item_index,
                guard.source.clone(),
                guard.source_identity,
                guard.source_kind,
                guard.target.clone(),
                state,
            )?)),
        }
    }

    fn resolve_current<P: PreflightProbe>(
        &mut self,
        conflict: Conflict,
        decision: ConflictDecision,
        probe: &P,
    ) -> Result<ConflictResolution, ConflictError> {
        match decision {
            ConflictDecision::Skip => Ok(ConflictResolution::Skipped {
                item_index: conflict.item_index,
            }),
            ConflictDecision::Cancel => {
                self.cancelled = true;
                Ok(ConflictResolution::Cancelled)
            }
            ConflictDecision::Replace => {
                let guard = existing_target_guard(&conflict);
                match conflict.kind {
                    ConflictKind::FileReplacement => Ok(ConflictResolution::Ready(
                        ConflictAction::ReplaceFile(guard),
                    )),
                    ConflictKind::DirectoryMerge => Ok(ConflictResolution::Ready(
                        ConflictAction::MergeDirectory(guard),
                    )),
                    ConflictKind::TypeMismatch => Err(ConflictError::ReplaceTypeMismatch),
                }
            }
            ConflictDecision::KeepBoth => {
                let target = keep_both_target(conflict.target.as_path(), probe)?;
                Ok(ConflictResolution::Ready(ConflictAction::KeepBoth(
                    ConflictGuard {
                        item_index: conflict.item_index,
                        source: conflict.source,
                        source_identity: conflict.source_identity,
                        source_kind: conflict.source_kind,
                        target: NativePath::new(target),
                        expected_target: None,
                    },
                )))
            }
        }
    }
}

impl ConflictAction {
    const fn guard(&self) -> &ConflictGuard {
        match self {
            Self::ReplaceFile(guard) | Self::MergeDirectory(guard) | Self::KeepBoth(guard) => guard,
        }
    }
}

enum ConflictRefresh {
    Unchanged,
    Vacated,
    Changed(Conflict),
}

fn refresh_conflict<P: PreflightProbe>(
    conflict: &Conflict,
    probe: &P,
) -> Result<ConflictRefresh, ConflictError> {
    let current = probe.inspect(conflict.target.as_path())?;
    match current {
        Some(state)
            if state.identity == Some(conflict.target_identity)
                && state.kind == conflict.target_kind =>
        {
            Ok(ConflictRefresh::Unchanged)
        }
        Some(state) => Ok(ConflictRefresh::Changed(make_conflict(
            conflict.item_index,
            conflict.source.clone(),
            conflict.source_identity,
            conflict.source_kind,
            conflict.target.clone(),
            state,
        )?)),
        None => Ok(ConflictRefresh::Vacated),
    }
}

fn make_conflict(
    item_index: usize,
    source: NativePath,
    source_identity: FileIdentity,
    source_kind: EntryKind,
    target: NativePath,
    target_state: PathPreflightState,
) -> Result<Conflict, ConflictError> {
    let target_identity = identity(target_state)?;
    if source_identity == target_identity {
        return Err(ConflictError::SameEntry);
    }
    let kind = match (
        source_kind == EntryKind::Directory,
        target_state.kind == EntryKind::Directory,
    ) {
        (true, true) => ConflictKind::DirectoryMerge,
        (false, false) => ConflictKind::FileReplacement,
        _ => ConflictKind::TypeMismatch,
    };
    Ok(Conflict {
        item_index,
        source,
        source_identity,
        source_kind,
        target,
        target_identity,
        target_kind: target_state.kind,
        kind,
    })
}

fn existing_target_guard(conflict: &Conflict) -> ConflictGuard {
    ConflictGuard {
        item_index: conflict.item_index,
        source: conflict.source.clone(),
        source_identity: conflict.source_identity,
        source_kind: conflict.source_kind,
        target: conflict.target.clone(),
        expected_target: Some((conflict.target_identity, conflict.target_kind)),
    }
}

fn identity(state: PathPreflightState) -> Result<FileIdentity, ConflictError> {
    state.identity.ok_or(ConflictError::IdentityUnavailable)
}

fn keep_both_target<P: PreflightProbe>(
    target: &Path,
    probe: &P,
) -> Result<std::path::PathBuf, ConflictError> {
    let parent = target
        .parent()
        .ok_or(ConflictError::KeepBothNameExhausted)?;
    let name = target
        .file_name()
        .ok_or(ConflictError::KeepBothNameExhausted)?;
    for attempt in 1..=MAX_KEEP_BOTH_ATTEMPTS {
        let candidate_name = alternate_name(name, target.extension(), attempt);
        let candidate = parent.join(candidate_name);
        if probe.inspect(&candidate)?.is_none() {
            return Ok(candidate);
        }
    }
    Err(ConflictError::KeepBothNameExhausted)
}

fn alternate_name(
    name: &std::ffi::OsStr,
    extension: Option<&std::ffi::OsStr>,
    attempt: u32,
) -> OsString {
    let path = Path::new(name);
    let stem = path.file_stem().unwrap_or(name);
    let mut result = OsString::from(stem);
    result.push(" copy");
    if attempt > 1 {
        result.push(format!(" {attempt}"));
    }
    if let Some(extension) = extension {
        result.push(".");
        result.push(extension);
    }
    result
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, fs, path::PathBuf};

    use tempfile::TempDir;

    use crate::StandardPreflightProbe;

    use super::*;

    fn conflict_from(resolution: ConflictResolution) -> Conflict {
        match resolution {
            ConflictResolution::AwaitingDecision(conflict) => conflict,
            _ => panic!("expected a conflict"),
        }
    }

    #[test]
    fn file_replace_and_directory_merge_are_distinct() {
        let temporary = TempDir::new().unwrap();
        let source_file = temporary.path().join("source.txt");
        let target_file = temporary.path().join("target.txt");
        let source_directory = temporary.path().join("source-folder");
        let target_directory = temporary.path().join("target-folder");
        fs::write(&source_file, b"source").unwrap();
        fs::write(&target_file, b"target").unwrap();
        fs::create_dir(&source_directory).unwrap();
        fs::create_dir(&target_directory).unwrap();
        let mut session = ConflictSession::new(OperationKind::Copy).unwrap();

        let file = conflict_from(
            session
                .encounter(
                    0,
                    NativePath::new(&source_file),
                    NativePath::new(&target_file),
                    &StandardPreflightProbe,
                )
                .unwrap(),
        );
        assert_eq!(file.kind(), ConflictKind::FileReplacement);
        assert!(matches!(
            session
                .decide(
                    file,
                    ConflictDecision::Replace,
                    ConflictScope::ThisConflict,
                    &StandardPreflightProbe,
                )
                .unwrap(),
            ConflictResolution::Ready(ConflictAction::ReplaceFile(_))
        ));

        let directory = conflict_from(
            session
                .encounter(
                    1,
                    NativePath::new(source_directory),
                    NativePath::new(target_directory),
                    &StandardPreflightProbe,
                )
                .unwrap(),
        );
        assert_eq!(directory.kind(), ConflictKind::DirectoryMerge);
        assert!(matches!(
            session
                .decide(
                    directory,
                    ConflictDecision::Replace,
                    ConflictScope::ThisConflict,
                    &StandardPreflightProbe,
                )
                .unwrap(),
            ConflictResolution::Ready(ConflictAction::MergeDirectory(_))
        ));
    }

    #[test]
    fn replace_never_treats_a_file_directory_mismatch_as_a_merge() {
        let temporary = TempDir::new().unwrap();
        let source = temporary.path().join("source-folder");
        let target = temporary.path().join("target.txt");
        fs::create_dir(&source).unwrap();
        fs::write(&target, b"target").unwrap();
        let mut session = ConflictSession::new(OperationKind::Move).unwrap();
        let conflict = conflict_from(
            session
                .encounter(
                    0,
                    NativePath::new(source),
                    NativePath::new(target),
                    &StandardPreflightProbe,
                )
                .unwrap(),
        );
        assert_eq!(conflict.kind(), ConflictKind::TypeMismatch);
        assert!(matches!(
            session.decide(
                conflict,
                ConflictDecision::Replace,
                ConflictScope::ThisConflict,
                &StandardPreflightProbe,
            ),
            Err(ConflictError::ReplaceTypeMismatch)
        ));
    }

    #[test]
    fn skip_keep_both_and_cancel_never_modify_existing_targets() {
        let temporary = TempDir::new().unwrap();
        let source = temporary.path().join("report.txt");
        let target = temporary.path().join("result.txt");
        fs::write(&source, b"source").unwrap();
        fs::write(&target, b"target").unwrap();

        for decision in [ConflictDecision::Skip, ConflictDecision::KeepBoth] {
            let mut session = ConflictSession::new(OperationKind::Move).unwrap();
            let conflict = conflict_from(
                session
                    .encounter(
                        0,
                        NativePath::new(&source),
                        NativePath::new(&target),
                        &StandardPreflightProbe,
                    )
                    .unwrap(),
            );
            let resolution = session
                .decide(
                    conflict,
                    decision,
                    ConflictScope::ThisConflict,
                    &StandardPreflightProbe,
                )
                .unwrap();
            if let ConflictResolution::Ready(action) = resolution {
                assert_eq!(
                    action.target().as_path(),
                    temporary.path().join("result copy.txt")
                );
            }
            assert_eq!(fs::read(&target).unwrap(), b"target");
            assert_eq!(fs::read(&source).unwrap(), b"source");
        }

        let mut session = ConflictSession::new(OperationKind::Rename).unwrap();
        let conflict = conflict_from(
            session
                .encounter(
                    0,
                    NativePath::new(&source),
                    NativePath::new(&target),
                    &StandardPreflightProbe,
                )
                .unwrap(),
        );
        assert!(matches!(
            session
                .decide(
                    conflict,
                    ConflictDecision::Cancel,
                    ConflictScope::ThisConflict,
                    &StandardPreflightProbe,
                )
                .unwrap(),
            ConflictResolution::Cancelled
        ));
        assert_eq!(fs::read(target).unwrap(), b"target");
    }

    #[test]
    fn remaining_policy_is_scoped_to_one_task() {
        let temporary = TempDir::new().unwrap();
        let source_one = temporary.path().join("one-source.txt");
        let target_one = temporary.path().join("one-target.txt");
        let source_two = temporary.path().join("two-source.txt");
        let target_two = temporary.path().join("two-target.txt");
        for path in [&source_one, &target_one, &source_two, &target_two] {
            fs::write(path, path.as_os_str().as_encoded_bytes()).unwrap();
        }
        let mut first_task = ConflictSession::new(OperationKind::Copy).unwrap();
        let first = conflict_from(
            first_task
                .encounter(
                    0,
                    NativePath::new(&source_one),
                    NativePath::new(&target_one),
                    &StandardPreflightProbe,
                )
                .unwrap(),
        );
        first_task
            .decide(
                first,
                ConflictDecision::Skip,
                ConflictScope::RemainingConflicts,
                &StandardPreflightProbe,
            )
            .unwrap();
        assert!(matches!(
            first_task
                .encounter(
                    1,
                    NativePath::new(&source_two),
                    NativePath::new(&target_two),
                    &StandardPreflightProbe,
                )
                .unwrap(),
            ConflictResolution::Skipped { item_index: 1 }
        ));

        let mut second_task = ConflictSession::new(OperationKind::Copy).unwrap();
        assert!(matches!(
            second_task
                .encounter(
                    0,
                    NativePath::new(source_two),
                    NativePath::new(target_two),
                    &StandardPreflightProbe,
                )
                .unwrap(),
            ConflictResolution::AwaitingDecision(_)
        ));
    }

    #[test]
    fn changed_target_requires_a_fresh_decision_before_policy_is_saved() {
        let temporary = TempDir::new().unwrap();
        let source = temporary.path().join("source.txt");
        let target = temporary.path().join("target.txt");
        fs::write(&source, b"source").unwrap();
        fs::write(&target, b"old target").unwrap();
        let mut session = ConflictSession::new(OperationKind::Copy).unwrap();
        let conflict = conflict_from(
            session
                .encounter(
                    0,
                    NativePath::new(&source),
                    NativePath::new(&target),
                    &StandardPreflightProbe,
                )
                .unwrap(),
        );
        fs::remove_file(&target).unwrap();
        fs::write(&target, b"new target").unwrap();

        assert!(matches!(
            session
                .decide(
                    conflict,
                    ConflictDecision::Replace,
                    ConflictScope::RemainingConflicts,
                    &StandardPreflightProbe,
                )
                .unwrap(),
            ConflictResolution::AwaitingDecision(_)
        ));
    }

    #[test]
    fn vanished_target_retries_without_saving_a_remaining_policy() {
        let temporary = TempDir::new().unwrap();
        let source = temporary.path().join("source.txt");
        let target = temporary.path().join("target.txt");
        fs::write(&source, b"source").unwrap();
        fs::write(&target, b"target").unwrap();
        let mut session = ConflictSession::new(OperationKind::Copy).unwrap();
        let conflict = conflict_from(
            session
                .encounter(
                    0,
                    NativePath::new(&source),
                    NativePath::new(&target),
                    &StandardPreflightProbe,
                )
                .unwrap(),
        );
        fs::remove_file(&target).unwrap();
        assert!(matches!(
            session
                .decide(
                    conflict,
                    ConflictDecision::Skip,
                    ConflictScope::RemainingConflicts,
                    &StandardPreflightProbe,
                )
                .unwrap(),
            ConflictResolution::NoConflict { .. }
        ));

        fs::write(&target, b"new target").unwrap();
        assert!(matches!(
            session
                .encounter(
                    0,
                    NativePath::new(source),
                    NativePath::new(target),
                    &StandardPreflightProbe,
                )
                .unwrap(),
            ConflictResolution::AwaitingDecision(_)
        ));
    }

    #[test]
    fn action_revalidation_detects_late_target_changes() {
        let temporary = TempDir::new().unwrap();
        let source = temporary.path().join("source.txt");
        let target = temporary.path().join("target.txt");
        fs::write(&source, b"source").unwrap();
        fs::write(&target, b"old target").unwrap();
        let mut session = ConflictSession::new(OperationKind::Copy).unwrap();
        let conflict = conflict_from(
            session
                .encounter(
                    0,
                    NativePath::new(&source),
                    NativePath::new(&target),
                    &StandardPreflightProbe,
                )
                .unwrap(),
        );
        let action = match session
            .decide(
                conflict,
                ConflictDecision::Replace,
                ConflictScope::ThisConflict,
                &StandardPreflightProbe,
            )
            .unwrap()
        {
            ConflictResolution::Ready(action) => action,
            _ => panic!("expected a ready action"),
        };
        fs::remove_file(&target).unwrap();
        fs::write(&target, b"new target").unwrap();

        assert!(matches!(
            session
                .revalidate_action(&action, &StandardPreflightProbe)
                .unwrap(),
            ActionReadiness::AwaitingDecision(_)
        ));
        assert_eq!(fs::read(target).unwrap(), b"new target");
    }

    #[test]
    fn keep_both_skips_occupied_candidate_names() {
        let source = NativePath::new("/source/report.txt");
        let target = NativePath::new("/target/report.txt");
        let source_state = state(1, EntryKind::File);
        let target_state = state(2, EntryKind::File);
        let occupied = state(3, EntryKind::File);
        let probe = FakeProbe(HashMap::from([
            (PathBuf::from("/source/report.txt"), source_state),
            (PathBuf::from("/target/report.txt"), target_state),
            (PathBuf::from("/target/report copy.txt"), occupied),
        ]));
        let mut session = ConflictSession::new(OperationKind::Rename).unwrap();
        let conflict = conflict_from(session.encounter(0, source, target, &probe).unwrap());
        let action = match session
            .decide(
                conflict,
                ConflictDecision::KeepBoth,
                ConflictScope::ThisConflict,
                &probe,
            )
            .unwrap()
        {
            ConflictResolution::Ready(action) => action,
            _ => panic!("expected keep-both action"),
        };
        assert_eq!(
            action.target().as_path(),
            Path::new("/target/report copy 2.txt")
        );
    }

    #[derive(Clone)]
    struct FakeProbe(HashMap<PathBuf, PathPreflightState>);

    impl PreflightProbe for FakeProbe {
        fn inspect(&self, path: &Path) -> Result<Option<PathPreflightState>, ProbeError> {
            Ok(self.0.get(path).copied())
        }
    }

    fn state(file_id: u128, kind: EntryKind) -> PathPreflightState {
        PathPreflightState {
            identity: Some(FileIdentity {
                volume_id: 1,
                file_id,
            }),
            kind,
            byte_len: 1,
            is_readable: true,
            is_writable: true,
        }
    }
}
