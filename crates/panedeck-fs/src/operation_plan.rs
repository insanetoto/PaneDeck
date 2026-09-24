use std::{
    error::Error,
    ffi::{OsStr, OsString},
    fmt, fs, io,
    path::{Component, Path, PathBuf},
};

use panedeck_domain::{EntryKind, NativePath};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct FileIdentity {
    pub volume_id: u64,
    pub file_id: u128,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PathPreflightState {
    pub identity: Option<FileIdentity>,
    pub kind: EntryKind,
    pub byte_len: u64,
    pub is_readable: bool,
    pub is_writable: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProbeError {
    PermissionDenied,
    InvalidPath,
    Io,
}

impl fmt::Display for ProbeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::PermissionDenied => "permission denied during preflight",
            Self::InvalidPath => "invalid path during preflight",
            Self::Io => "I/O error during preflight",
        })
    }
}

impl Error for ProbeError {}

/// Read-only facts required to create or revalidate an operation plan.
///
/// There is intentionally no mutation method on this trait.
pub trait PreflightProbe {
    fn inspect(&self, path: &Path) -> Result<Option<PathPreflightState>, ProbeError>;

    fn estimated_copy_bytes(&self, path: &Path) -> Result<Option<u64>, ProbeError> {
        Ok(self.inspect(path)?.map(|state| state.byte_len))
    }

    fn available_space(&self, _directory: &Path) -> Result<Option<u64>, ProbeError> {
        Ok(None)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct StandardPreflightProbe;

impl PreflightProbe for StandardPreflightProbe {
    fn inspect(&self, path: &Path) -> Result<Option<PathPreflightState>, ProbeError> {
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(map_probe_error(error.kind())),
        };
        let file_type = metadata.file_type();
        let kind = if file_type.is_symlink() {
            EntryKind::Symlink
        } else if file_type.is_dir() {
            EntryKind::Directory
        } else if file_type.is_file() {
            EntryKind::File
        } else {
            EntryKind::Other
        };
        let (is_readable, is_writable) = permission_bits(&metadata);

        Ok(Some(PathPreflightState {
            identity: file_identity(&metadata),
            kind,
            byte_len: metadata.len(),
            is_readable,
            is_writable,
        }))
    }

    fn estimated_copy_bytes(&self, path: &Path) -> Result<Option<u64>, ProbeError> {
        estimate_copy_bytes(path).map(Some)
    }

    fn available_space(&self, directory: &Path) -> Result<Option<u64>, ProbeError> {
        fs2::available_space(directory)
            .map(Some)
            .map_err(|error| map_probe_error(error.kind()))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OperationRequest {
    Copy {
        sources: Vec<NativePath>,
        destination_directory: NativePath,
    },
    Move {
        sources: Vec<NativePath>,
        destination_directory: NativePath,
    },
    Rename {
        source: NativePath,
        new_name: OsString,
    },
    CreateDirectory {
        parent: NativePath,
        name: OsString,
    },
    Trash {
        sources: Vec<NativePath>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OperationKind {
    Copy,
    Move,
    Rename,
    CreateDirectory,
    Trash,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlanValidationErrorKind {
    EmptySources,
    InvalidName,
    MissingSource { source_index: usize },
    MissingDestination,
    DestinationNotDirectory,
    SourceNotReadable { source_index: usize },
    ParentNotWritable,
    IdentityUnavailable,
    SamePath { source_index: usize },
    DirectoryIntoItself { source_index: usize },
    Conflict { target_index: usize },
    InsufficientSpace { required: u64, available: u64 },
    SourceChanged { source_index: usize },
    ParentChanged { target_index: usize },
    TargetChanged { target_index: usize },
    ProbeFailed(ProbeError),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlanValidationError {
    kind: PlanValidationErrorKind,
}

impl PlanValidationError {
    #[must_use]
    pub const fn new(kind: PlanValidationErrorKind) -> Self {
        Self { kind }
    }

    #[must_use]
    pub const fn kind(&self) -> &PlanValidationErrorKind {
        &self.kind
    }
}

impl fmt::Display for PlanValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "operation plan validation failed: {:?}",
            self.kind
        )
    }
}

impl Error for PlanValidationError {}

impl From<ProbeError> for PlanValidationError {
    fn from(value: ProbeError) -> Self {
        Self::new(PlanValidationErrorKind::ProbeFailed(value))
    }
}

#[derive(Clone, Debug)]
pub struct PlannedSource {
    path: NativePath,
    identity: FileIdentity,
    kind: EntryKind,
    byte_len: u64,
    parent: Option<(NativePath, FileIdentity)>,
}

impl PlannedSource {
    #[must_use]
    pub const fn native_path(&self) -> &NativePath {
        &self.path
    }

    #[must_use]
    pub const fn identity(&self) -> FileIdentity {
        self.identity
    }

    #[must_use]
    pub const fn kind(&self) -> EntryKind {
        self.kind
    }

    #[must_use]
    pub const fn byte_len(&self) -> u64 {
        self.byte_len
    }
}

#[derive(Clone, Debug)]
pub struct PlannedTarget {
    path: NativePath,
    parent: NativePath,
    parent_identity: FileIdentity,
    expected_existing_identity: Option<FileIdentity>,
}

impl PlannedTarget {
    #[must_use]
    pub const fn native_path(&self) -> &NativePath {
        &self.path
    }

    #[must_use]
    pub const fn expected_existing_identity(&self) -> Option<FileIdentity> {
        self.expected_existing_identity
    }

    #[must_use]
    pub const fn parent_identity(&self) -> FileIdentity {
        self.parent_identity
    }
}

#[derive(Clone, Debug)]
struct SpaceRequirement {
    directory: NativePath,
    required: u64,
}

/// Immutable result of successful preflight validation.
#[derive(Clone, Debug)]
pub struct OperationPlan {
    kind: OperationKind,
    sources: Vec<PlannedSource>,
    targets: Vec<PlannedTarget>,
    space_requirements: Vec<SpaceRequirement>,
}

impl OperationPlan {
    #[must_use]
    pub const fn kind(&self) -> OperationKind {
        self.kind
    }

    #[must_use]
    pub fn sources(&self) -> &[PlannedSource] {
        &self.sources
    }

    #[must_use]
    pub fn targets(&self) -> &[PlannedTarget] {
        &self.targets
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct OperationPlanner;

impl OperationPlanner {
    pub fn plan<P: PreflightProbe>(
        &self,
        request: OperationRequest,
        probe: &P,
    ) -> Result<OperationPlan, PlanValidationError> {
        match request {
            OperationRequest::Copy {
                sources,
                destination_directory,
            } => self.plan_transfer(OperationKind::Copy, sources, destination_directory, probe),
            OperationRequest::Move {
                sources,
                destination_directory,
            } => self.plan_transfer(OperationKind::Move, sources, destination_directory, probe),
            OperationRequest::Rename { source, new_name } => {
                self.plan_rename(source, new_name, probe)
            }
            OperationRequest::CreateDirectory { parent, name } => {
                self.plan_create_directory(parent, name, probe)
            }
            OperationRequest::Trash { sources } => self.plan_trash(sources, probe),
        }
    }

    pub fn revalidate<P: PreflightProbe>(
        &self,
        plan: &OperationPlan,
        probe: &P,
    ) -> Result<(), PlanValidationError> {
        for (source_index, source) in plan.sources.iter().enumerate() {
            let state = probe.inspect(source.path.as_path())?.ok_or_else(|| {
                PlanValidationError::new(PlanValidationErrorKind::SourceChanged { source_index })
            })?;
            if state.identity != Some(source.identity) {
                return Err(PlanValidationError::new(
                    PlanValidationErrorKind::SourceChanged { source_index },
                ));
            }
            if matches!(plan.kind, OperationKind::Copy | OperationKind::Move) && !state.is_readable
            {
                return Err(PlanValidationError::new(
                    PlanValidationErrorKind::SourceChanged { source_index },
                ));
            }
            if let Some((parent, expected_identity)) = &source.parent {
                let parent_state = probe.inspect(parent.as_path())?.ok_or_else(|| {
                    PlanValidationError::new(PlanValidationErrorKind::SourceChanged {
                        source_index,
                    })
                })?;
                if parent_state.identity != Some(*expected_identity) || !parent_state.is_writable {
                    return Err(PlanValidationError::new(
                        PlanValidationErrorKind::SourceChanged { source_index },
                    ));
                }
            }
        }

        for (target_index, target) in plan.targets.iter().enumerate() {
            let parent_state = probe.inspect(target.parent.as_path())?.ok_or_else(|| {
                PlanValidationError::new(PlanValidationErrorKind::ParentChanged { target_index })
            })?;
            if parent_state.identity != Some(target.parent_identity) || !parent_state.is_writable {
                return Err(PlanValidationError::new(
                    PlanValidationErrorKind::ParentChanged { target_index },
                ));
            }
            let target_identity = probe
                .inspect(target.path.as_path())?
                .and_then(|state| state.identity);
            if target_identity != target.expected_existing_identity {
                return Err(PlanValidationError::new(
                    PlanValidationErrorKind::TargetChanged { target_index },
                ));
            }
        }

        for requirement in &plan.space_requirements {
            if let Some(available) = probe.available_space(requirement.directory.as_path())? {
                if available < requirement.required {
                    return Err(PlanValidationError::new(
                        PlanValidationErrorKind::InsufficientSpace {
                            required: requirement.required,
                            available,
                        },
                    ));
                }
            }
        }
        Ok(())
    }

    fn plan_transfer<P: PreflightProbe>(
        &self,
        kind: OperationKind,
        sources: Vec<NativePath>,
        destination_directory: NativePath,
        probe: &P,
    ) -> Result<OperationPlan, PlanValidationError> {
        if sources.is_empty() {
            return Err(PlanValidationError::new(
                PlanValidationErrorKind::EmptySources,
            ));
        }
        let destination_state = required_destination(probe, &destination_directory)?;
        let destination_identity = required_identity(destination_state)?;
        if !destination_state.is_writable {
            return Err(PlanValidationError::new(
                PlanValidationErrorKind::ParentNotWritable,
            ));
        }

        let mut planned_sources = Vec::with_capacity(sources.len());
        let mut planned_targets = Vec::with_capacity(sources.len());
        let mut required_space = Some(0_u64);

        for (source_index, source) in sources.into_iter().enumerate() {
            let source_state = required_source(probe, &source, source_index)?;
            if !source_state.is_readable {
                return Err(PlanValidationError::new(
                    PlanValidationErrorKind::SourceNotReadable { source_index },
                ));
            }
            let source_identity = required_identity(source_state)?;
            let file_name = source
                .as_path()
                .file_name()
                .ok_or_else(|| PlanValidationError::new(PlanValidationErrorKind::InvalidName))?;
            let target = NativePath::new(destination_directory.as_path().join(file_name));

            if target.as_path() == source.as_path() {
                return Err(PlanValidationError::new(
                    PlanValidationErrorKind::SamePath { source_index },
                ));
            }
            if source_state.kind == EntryKind::Directory
                && destination_directory
                    .as_path()
                    .starts_with(source.as_path())
            {
                return Err(PlanValidationError::new(
                    PlanValidationErrorKind::DirectoryIntoItself { source_index },
                ));
            }
            if let Some(target_state) = probe.inspect(target.as_path())? {
                if target_state.identity == Some(source_identity) {
                    return Err(PlanValidationError::new(
                        PlanValidationErrorKind::SamePath { source_index },
                    ));
                }
                return Err(PlanValidationError::new(
                    PlanValidationErrorKind::Conflict {
                        target_index: planned_targets.len(),
                    },
                ));
            }

            let source_parent = if kind == OperationKind::Move {
                Some(required_writable_parent(probe, &source)?)
            } else {
                None
            };

            let needs_copy_space = kind == OperationKind::Copy
                || source_identity.volume_id != destination_identity.volume_id;
            if needs_copy_space {
                required_space = match (
                    required_space,
                    probe.estimated_copy_bytes(source.as_path())?,
                ) {
                    (Some(total), Some(bytes)) => total.checked_add(bytes),
                    _ => None,
                };
            }

            planned_sources.push(PlannedSource {
                path: source,
                identity: source_identity,
                kind: source_state.kind,
                byte_len: source_state.byte_len,
                parent: source_parent,
            });
            planned_targets.push(PlannedTarget {
                path: target,
                parent: destination_directory.clone(),
                parent_identity: destination_identity,
                expected_existing_identity: None,
            });
        }

        let space_requirements = space_requirement(
            probe,
            destination_directory,
            required_space.filter(|bytes| *bytes > 0),
        )?;
        Ok(OperationPlan {
            kind,
            sources: planned_sources,
            targets: planned_targets,
            space_requirements,
        })
    }

    fn plan_rename<P: PreflightProbe>(
        &self,
        source: NativePath,
        new_name: OsString,
        probe: &P,
    ) -> Result<OperationPlan, PlanValidationError> {
        validate_name(&new_name)?;
        let source_state = required_source(probe, &source, 0)?;
        let source_identity = required_identity(source_state)?;
        let (parent, parent_identity) = required_writable_parent(probe, &source)?;
        let target = NativePath::new(parent.as_path().join(&new_name));
        if target.as_path() == source.as_path() {
            return Err(PlanValidationError::new(
                PlanValidationErrorKind::SamePath { source_index: 0 },
            ));
        }
        let expected_existing_identity =
            if let Some(target_state) = probe.inspect(target.as_path())? {
                if target_state.identity == Some(source_identity) {
                    Some(source_identity)
                } else {
                    return Err(PlanValidationError::new(
                        PlanValidationErrorKind::Conflict { target_index: 0 },
                    ));
                }
            } else {
                None
            };

        Ok(OperationPlan {
            kind: OperationKind::Rename,
            sources: vec![PlannedSource {
                path: source,
                identity: source_identity,
                kind: source_state.kind,
                byte_len: source_state.byte_len,
                parent: Some((parent.clone(), parent_identity)),
            }],
            targets: vec![PlannedTarget {
                path: target,
                parent,
                parent_identity,
                expected_existing_identity,
            }],
            space_requirements: Vec::new(),
        })
    }

    fn plan_create_directory<P: PreflightProbe>(
        &self,
        parent: NativePath,
        name: OsString,
        probe: &P,
    ) -> Result<OperationPlan, PlanValidationError> {
        validate_name(&name)?;
        let parent_state = required_destination(probe, &parent)?;
        let parent_identity = required_identity(parent_state)?;
        if !parent_state.is_writable {
            return Err(PlanValidationError::new(
                PlanValidationErrorKind::ParentNotWritable,
            ));
        }
        let target = NativePath::new(parent.as_path().join(name));
        if probe.inspect(target.as_path())?.is_some() {
            return Err(PlanValidationError::new(
                PlanValidationErrorKind::Conflict { target_index: 0 },
            ));
        }

        Ok(OperationPlan {
            kind: OperationKind::CreateDirectory,
            sources: Vec::new(),
            targets: vec![PlannedTarget {
                path: target,
                parent,
                parent_identity,
                expected_existing_identity: None,
            }],
            space_requirements: Vec::new(),
        })
    }

    fn plan_trash<P: PreflightProbe>(
        &self,
        sources: Vec<NativePath>,
        probe: &P,
    ) -> Result<OperationPlan, PlanValidationError> {
        if sources.is_empty() {
            return Err(PlanValidationError::new(
                PlanValidationErrorKind::EmptySources,
            ));
        }
        let mut planned_sources = Vec::with_capacity(sources.len());
        for (source_index, source) in sources.into_iter().enumerate() {
            let state = required_source(probe, &source, source_index)?;
            let identity = required_identity(state)?;
            let parent = required_writable_parent(probe, &source)?;
            planned_sources.push(PlannedSource {
                path: source,
                identity,
                kind: state.kind,
                byte_len: state.byte_len,
                parent: Some(parent),
            });
        }
        Ok(OperationPlan {
            kind: OperationKind::Trash,
            sources: planned_sources,
            targets: Vec::new(),
            space_requirements: Vec::new(),
        })
    }
}

fn required_source<P: PreflightProbe>(
    probe: &P,
    source: &NativePath,
    source_index: usize,
) -> Result<PathPreflightState, PlanValidationError> {
    probe.inspect(source.as_path())?.ok_or_else(|| {
        PlanValidationError::new(PlanValidationErrorKind::MissingSource { source_index })
    })
}

fn required_destination<P: PreflightProbe>(
    probe: &P,
    destination: &NativePath,
) -> Result<PathPreflightState, PlanValidationError> {
    let state = probe
        .inspect(destination.as_path())?
        .ok_or_else(|| PlanValidationError::new(PlanValidationErrorKind::MissingDestination))?;
    if state.kind != EntryKind::Directory {
        return Err(PlanValidationError::new(
            PlanValidationErrorKind::DestinationNotDirectory,
        ));
    }
    Ok(state)
}

fn required_writable_parent<P: PreflightProbe>(
    probe: &P,
    path: &NativePath,
) -> Result<(NativePath, FileIdentity), PlanValidationError> {
    let parent = path
        .as_path()
        .parent()
        .ok_or_else(|| PlanValidationError::new(PlanValidationErrorKind::InvalidName))?;
    let parent = NativePath::new(parent);
    let state = required_destination(probe, &parent)?;
    if !state.is_writable {
        return Err(PlanValidationError::new(
            PlanValidationErrorKind::ParentNotWritable,
        ));
    }
    Ok((parent, required_identity(state)?))
}

fn required_identity(state: PathPreflightState) -> Result<FileIdentity, PlanValidationError> {
    state
        .identity
        .ok_or_else(|| PlanValidationError::new(PlanValidationErrorKind::IdentityUnavailable))
}

fn validate_name(name: &OsStr) -> Result<(), PlanValidationError> {
    let mut components = Path::new(name).components();
    let valid = matches!(components.next(), Some(Component::Normal(_)))
        && components.next().is_none()
        && !contains_nul(name);
    if valid {
        Ok(())
    } else {
        Err(PlanValidationError::new(
            PlanValidationErrorKind::InvalidName,
        ))
    }
}

#[cfg(unix)]
fn contains_nul(name: &OsStr) -> bool {
    use std::os::unix::ffi::OsStrExt;
    name.as_bytes().contains(&0)
}

#[cfg(not(unix))]
fn contains_nul(name: &OsStr) -> bool {
    name.to_string_lossy().contains('\0')
}

fn space_requirement<P: PreflightProbe>(
    probe: &P,
    directory: NativePath,
    required: Option<u64>,
) -> Result<Vec<SpaceRequirement>, PlanValidationError> {
    let Some(required) = required else {
        return Ok(Vec::new());
    };
    if let Some(available) = probe.available_space(directory.as_path())? {
        if available < required {
            return Err(PlanValidationError::new(
                PlanValidationErrorKind::InsufficientSpace {
                    required,
                    available,
                },
            ));
        }
    }
    Ok(vec![SpaceRequirement {
        directory,
        required,
    }])
}

fn estimate_copy_bytes(path: &Path) -> Result<u64, ProbeError> {
    let mut total = 0_u64;
    let mut pending = vec![PathBuf::from(path)];
    while let Some(current) = pending.pop() {
        let metadata =
            fs::symlink_metadata(&current).map_err(|error| map_probe_error(error.kind()))?;
        if metadata.is_dir() && !metadata.file_type().is_symlink() {
            for child in fs::read_dir(&current).map_err(|error| map_probe_error(error.kind()))? {
                pending.push(child.map_err(|error| map_probe_error(error.kind()))?.path());
            }
        } else {
            total = total.checked_add(metadata.len()).ok_or(ProbeError::Io)?;
        }
    }
    Ok(total)
}

#[cfg(unix)]
fn file_identity(metadata: &fs::Metadata) -> Option<FileIdentity> {
    use std::os::unix::fs::MetadataExt;
    Some(FileIdentity {
        volume_id: metadata.dev(),
        file_id: u128::from(metadata.ino()),
    })
}

#[cfg(not(unix))]
fn file_identity(_metadata: &fs::Metadata) -> Option<FileIdentity> {
    None
}

#[cfg(unix)]
fn permission_bits(metadata: &fs::Metadata) -> (bool, bool) {
    use std::os::unix::fs::PermissionsExt;
    let mode = metadata.permissions().mode();
    let is_directory = metadata.is_dir();
    let readable_mask = if is_directory { 0o555 } else { 0o444 };
    (mode & readable_mask != 0, mode & 0o222 != 0)
}

#[cfg(not(unix))]
fn permission_bits(metadata: &fs::Metadata) -> (bool, bool) {
    (true, !metadata.permissions().readonly())
}

const fn map_probe_error(kind: io::ErrorKind) -> ProbeError {
    match kind {
        io::ErrorKind::PermissionDenied => ProbeError::PermissionDenied,
        io::ErrorKind::InvalidData | io::ErrorKind::InvalidInput => ProbeError::InvalidPath,
        _ => ProbeError::Io,
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, collections::HashMap, fs::File, io::Write};

    use tempfile::TempDir;

    use super::*;

    #[derive(Default)]
    struct FakeProbe {
        paths: HashMap<PathBuf, PathPreflightState>,
        estimated_sizes: HashMap<PathBuf, u64>,
        spaces: HashMap<PathBuf, u64>,
        inspections: Cell<usize>,
    }

    impl PreflightProbe for FakeProbe {
        fn inspect(&self, path: &Path) -> Result<Option<PathPreflightState>, ProbeError> {
            self.inspections.set(self.inspections.get() + 1);
            Ok(self.paths.get(path).copied())
        }

        fn estimated_copy_bytes(&self, path: &Path) -> Result<Option<u64>, ProbeError> {
            Ok(self.estimated_sizes.get(path).copied())
        }

        fn available_space(&self, directory: &Path) -> Result<Option<u64>, ProbeError> {
            Ok(self.spaces.get(directory).copied())
        }
    }

    fn identity(volume_id: u64, file_id: u128) -> FileIdentity {
        FileIdentity { volume_id, file_id }
    }

    fn state(volume_id: u64, file_id: u128, kind: EntryKind) -> PathPreflightState {
        PathPreflightState {
            identity: Some(identity(volume_id, file_id)),
            kind,
            byte_len: 10,
            is_readable: true,
            is_writable: true,
        }
    }

    fn base_probe() -> FakeProbe {
        let mut probe = FakeProbe::default();
        probe
            .paths
            .insert(PathBuf::from("/source"), state(1, 1, EntryKind::Directory));
        probe
            .paths
            .insert(PathBuf::from("/source/a"), state(1, 2, EntryKind::File));
        probe.paths.insert(
            PathBuf::from("/destination"),
            state(2, 3, EntryKind::Directory),
        );
        probe.estimated_sizes.insert(PathBuf::from("/source/a"), 10);
        probe.spaces.insert(PathBuf::from("/destination"), 100);
        probe
    }

    #[test]
    fn creates_immutable_copy_plan_with_identity_and_space_snapshot() {
        let probe = base_probe();
        let plan = OperationPlanner
            .plan(
                OperationRequest::Copy {
                    sources: vec![NativePath::new("/source/a")],
                    destination_directory: NativePath::new("/destination"),
                },
                &probe,
            )
            .unwrap();

        assert_eq!(plan.kind(), OperationKind::Copy);
        assert_eq!(plan.sources().len(), 1);
        assert_eq!(plan.sources()[0].identity(), identity(1, 2));
        assert_eq!(plan.targets().len(), 1);
        assert!(OperationPlanner.revalidate(&plan, &probe).is_ok());
    }

    #[test]
    fn rejects_conflict_same_path_directory_into_itself_and_insufficient_space() {
        let planner = OperationPlanner;

        let mut same_path = base_probe();
        same_path
            .paths
            .insert(PathBuf::from("/source"), state(1, 1, EntryKind::Directory));
        let error = planner
            .plan(
                OperationRequest::Copy {
                    sources: vec![NativePath::new("/source/a")],
                    destination_directory: NativePath::new("/source"),
                },
                &same_path,
            )
            .unwrap_err();
        assert!(matches!(
            error.kind(),
            PlanValidationErrorKind::SamePath { .. }
        ));

        let mut into_itself = base_probe();
        into_itself.paths.insert(
            PathBuf::from("/source/child"),
            state(1, 4, EntryKind::Directory),
        );
        let error = planner
            .plan(
                OperationRequest::Move {
                    sources: vec![NativePath::new("/source")],
                    destination_directory: NativePath::new("/source/child"),
                },
                &into_itself,
            )
            .unwrap_err();
        assert!(matches!(
            error.kind(),
            PlanValidationErrorKind::DirectoryIntoItself { .. }
        ));

        let mut conflict = base_probe();
        conflict.paths.insert(
            PathBuf::from("/destination/a"),
            state(2, 5, EntryKind::File),
        );
        let error = planner
            .plan(
                OperationRequest::Copy {
                    sources: vec![NativePath::new("/source/a")],
                    destination_directory: NativePath::new("/destination"),
                },
                &conflict,
            )
            .unwrap_err();
        assert!(matches!(
            error.kind(),
            PlanValidationErrorKind::Conflict { .. }
        ));

        let mut no_space = base_probe();
        no_space.spaces.insert(PathBuf::from("/destination"), 5);
        let error = planner
            .plan(
                OperationRequest::Copy {
                    sources: vec![NativePath::new("/source/a")],
                    destination_directory: NativePath::new("/destination"),
                },
                &no_space,
            )
            .unwrap_err();
        assert_eq!(
            error.kind(),
            &PlanValidationErrorKind::InsufficientSpace {
                required: 10,
                available: 5,
            }
        );
    }

    #[test]
    fn checks_permissions_and_revalidates_changed_disk_state() {
        let mut denied = base_probe();
        denied
            .paths
            .get_mut(Path::new("/destination"))
            .unwrap()
            .is_writable = false;
        let error = OperationPlanner
            .plan(
                OperationRequest::Copy {
                    sources: vec![NativePath::new("/source/a")],
                    destination_directory: NativePath::new("/destination"),
                },
                &denied,
            )
            .unwrap_err();
        assert_eq!(error.kind(), &PlanValidationErrorKind::ParentNotWritable);

        let original = base_probe();
        let plan = OperationPlanner
            .plan(
                OperationRequest::Copy {
                    sources: vec![NativePath::new("/source/a")],
                    destination_directory: NativePath::new("/destination"),
                },
                &original,
            )
            .unwrap();
        let mut changed = base_probe();
        changed.paths.insert(
            PathBuf::from("/destination/a"),
            state(2, 99, EntryKind::File),
        );
        let error = OperationPlanner.revalidate(&plan, &changed).unwrap_err();
        assert!(matches!(
            error.kind(),
            PlanValidationErrorKind::TargetChanged { .. }
        ));

        let mut replaced_source = base_probe();
        replaced_source
            .paths
            .insert(PathBuf::from("/source/a"), state(1, 100, EntryKind::File));
        let error = OperationPlanner
            .revalidate(&plan, &replaced_source)
            .unwrap_err();
        assert!(matches!(
            error.kind(),
            PlanValidationErrorKind::SourceChanged { .. }
        ));
    }

    #[test]
    fn same_volume_move_does_not_require_copy_space() {
        let mut probe = base_probe();
        probe.paths.insert(
            PathBuf::from("/destination"),
            state(1, 3, EntryKind::Directory),
        );
        probe.spaces.insert(PathBuf::from("/destination"), 0);

        let plan = OperationPlanner
            .plan(
                OperationRequest::Move {
                    sources: vec![NativePath::new("/source/a")],
                    destination_directory: NativePath::new("/destination"),
                },
                &probe,
            )
            .unwrap();

        assert_eq!(plan.kind(), OperationKind::Move);
        assert!(OperationPlanner.revalidate(&plan, &probe).is_ok());
    }

    #[test]
    fn plans_move_rename_create_directory_and_trash() {
        let probe = base_probe();
        let planner = OperationPlanner;
        let move_plan = planner
            .plan(
                OperationRequest::Move {
                    sources: vec![NativePath::new("/source/a")],
                    destination_directory: NativePath::new("/destination"),
                },
                &probe,
            )
            .unwrap();
        assert_eq!(move_plan.kind(), OperationKind::Move);

        let rename_plan = planner
            .plan(
                OperationRequest::Rename {
                    source: NativePath::new("/source/a"),
                    new_name: OsString::from("renamed"),
                },
                &probe,
            )
            .unwrap();
        assert_eq!(rename_plan.kind(), OperationKind::Rename);

        let create_plan = planner
            .plan(
                OperationRequest::CreateDirectory {
                    parent: NativePath::new("/destination"),
                    name: OsString::from("new-folder"),
                },
                &probe,
            )
            .unwrap();
        assert_eq!(create_plan.kind(), OperationKind::CreateDirectory);

        let trash_plan = planner
            .plan(
                OperationRequest::Trash {
                    sources: vec![NativePath::new("/source/a")],
                },
                &probe,
            )
            .unwrap();
        assert_eq!(trash_plan.kind(), OperationKind::Trash);
    }

    #[test]
    fn invalid_real_filesystem_plan_does_not_modify_files() {
        let temp = TempDir::new().unwrap();
        let source_directory = temp.path().join("source");
        let destination_directory = temp.path().join("destination");
        fs::create_dir(&source_directory).unwrap();
        fs::create_dir(&destination_directory).unwrap();
        let source = source_directory.join("data.txt");
        let target = destination_directory.join("data.txt");
        File::create(&source).unwrap().write_all(b"source").unwrap();
        File::create(&target).unwrap().write_all(b"target").unwrap();

        let result = OperationPlanner.plan(
            OperationRequest::Copy {
                sources: vec![NativePath::new(&source)],
                destination_directory: NativePath::new(&destination_directory),
            },
            &StandardPreflightProbe,
        );

        assert!(matches!(
            result.unwrap_err().kind(),
            PlanValidationErrorKind::Conflict { .. }
        ));
        assert_eq!(fs::read(&source).unwrap(), b"source");
        assert_eq!(fs::read(&target).unwrap(), b"target");
    }

    #[test]
    fn standard_probe_reads_available_space_without_mutating_the_directory() {
        let temp = TempDir::new().unwrap();
        let before: Vec<_> = fs::read_dir(temp.path()).unwrap().collect();
        let available = StandardPreflightProbe
            .available_space(temp.path())
            .unwrap()
            .expect("macOS exposes available filesystem space");
        let after: Vec<_> = fs::read_dir(temp.path()).unwrap().collect();

        assert!(available > 0);
        assert_eq!(before.len(), after.len());
    }
}
