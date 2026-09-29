//! Deterministic, read-only fault injection for operation planning and revalidation.
//!
//! This adapter deliberately wraps the same [`PreflightProbe`] contract used by
//! production. It never mutates the file system, so safety failures can be
//! reproduced entirely inside a temporary fixture.

use std::path::{Path, PathBuf};

use crate::{FileIdentity, PathPreflightState, PreflightProbe, ProbeError};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InjectedFault {
    Missing {
        path: PathBuf,
    },
    PermissionDenied {
        path: PathBuf,
    },
    Io {
        path: PathBuf,
    },
    ReadOnly {
        path: PathBuf,
    },
    InsufficientSpace {
        directory: PathBuf,
        available: u64,
    },
    DifferentVolume {
        path: PathBuf,
        volume_id: u64,
    },
    ChangedIdentity {
        path: PathBuf,
        identity: FileIdentity,
    },
}

#[derive(Clone, Debug)]
pub struct FaultInjectingProbe<P> {
    inner: P,
    faults: Vec<InjectedFault>,
}

impl<P> FaultInjectingProbe<P> {
    #[must_use]
    pub fn new(inner: P, faults: impl IntoIterator<Item = InjectedFault>) -> Self {
        Self {
            inner,
            faults: faults.into_iter().collect(),
        }
    }

    fn fault_for_path(&self, path: &Path) -> Option<&InjectedFault> {
        self.faults.iter().find(|fault| match fault {
            InjectedFault::Missing { path: affected }
            | InjectedFault::PermissionDenied { path: affected }
            | InjectedFault::Io { path: affected }
            | InjectedFault::ReadOnly { path: affected }
            | InjectedFault::DifferentVolume { path: affected, .. }
            | InjectedFault::ChangedIdentity { path: affected, .. } => affected == path,
            InjectedFault::InsufficientSpace { .. } => false,
        })
    }
}

impl<P: PreflightProbe> PreflightProbe for FaultInjectingProbe<P> {
    fn inspect(&self, path: &Path) -> Result<Option<PathPreflightState>, ProbeError> {
        let fault = self.fault_for_path(path);
        match fault {
            Some(InjectedFault::Missing { .. }) => return Ok(None),
            Some(InjectedFault::PermissionDenied { .. }) => {
                return Err(ProbeError::PermissionDenied)
            }
            Some(InjectedFault::Io { .. }) => return Err(ProbeError::Io),
            _ => {}
        }

        let mut state = self.inner.inspect(path)?;
        if let Some(value) = &mut state {
            match fault {
                Some(InjectedFault::ReadOnly { .. }) => value.is_writable = false,
                Some(InjectedFault::DifferentVolume { volume_id, .. }) => {
                    if let Some(identity) = &mut value.identity {
                        identity.volume_id = *volume_id;
                    }
                }
                Some(InjectedFault::ChangedIdentity { identity, .. }) => {
                    value.identity = Some(*identity);
                }
                _ => {}
            }
        }
        Ok(state)
    }

    fn estimated_copy_bytes(&self, path: &Path) -> Result<Option<u64>, ProbeError> {
        match self.fault_for_path(path) {
            Some(InjectedFault::Missing { .. }) => Ok(None),
            Some(InjectedFault::PermissionDenied { .. }) => Err(ProbeError::PermissionDenied),
            Some(InjectedFault::Io { .. }) => Err(ProbeError::Io),
            _ => self.inner.estimated_copy_bytes(path),
        }
    }

    fn available_space(&self, directory: &Path) -> Result<Option<u64>, ProbeError> {
        if let Some(InjectedFault::InsufficientSpace { available, .. }) =
            self.faults.iter().find(|fault| {
                matches!(fault, InjectedFault::InsufficientSpace { directory: affected, .. } if affected == directory)
            })
        {
            return Ok(Some(*available));
        }
        self.inner.available_space(directory)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use panedeck_domain::NativePath;
    use tempfile::TempDir;

    use crate::{
        OperationPlanner, OperationRequest, PlanValidationErrorKind, StandardPreflightProbe,
    };

    use super::*;

    #[test]
    fn injects_space_disconnect_and_external_identity_changes_without_mutation() {
        let temporary = TempDir::new().unwrap();
        let source = temporary.path().join("source.bin");
        let destination = temporary.path().join("destination");
        fs::write(&source, vec![7_u8; 16]).unwrap();
        fs::create_dir(&destination).unwrap();

        let no_space = FaultInjectingProbe::new(
            StandardPreflightProbe,
            [InjectedFault::InsufficientSpace {
                directory: destination.clone(),
                available: 1,
            }],
        );
        let error = OperationPlanner
            .plan(
                OperationRequest::Copy {
                    sources: vec![NativePath::new(&source)],
                    destination_directory: NativePath::new(&destination),
                },
                &no_space,
            )
            .unwrap_err();
        assert!(matches!(
            error.kind(),
            PlanValidationErrorKind::InsufficientSpace { .. }
        ));

        let plan = OperationPlanner
            .plan(
                OperationRequest::Copy {
                    sources: vec![NativePath::new(&source)],
                    destination_directory: NativePath::new(&destination),
                },
                &StandardPreflightProbe,
            )
            .unwrap();
        let disconnected = FaultInjectingProbe::new(
            StandardPreflightProbe,
            [InjectedFault::Missing {
                path: destination.clone(),
            }],
        );
        assert!(matches!(
            OperationPlanner
                .revalidate(&plan, &disconnected)
                .unwrap_err()
                .kind(),
            PlanValidationErrorKind::ParentChanged { .. }
        ));

        let original_identity = StandardPreflightProbe
            .inspect(&source)
            .unwrap()
            .unwrap()
            .identity
            .unwrap();
        let changed = FaultInjectingProbe::new(
            StandardPreflightProbe,
            [InjectedFault::ChangedIdentity {
                path: source.clone(),
                identity: FileIdentity {
                    volume_id: original_identity.volume_id,
                    file_id: original_identity.file_id.wrapping_add(1),
                },
            }],
        );
        assert!(matches!(
            OperationPlanner
                .revalidate(&plan, &changed)
                .unwrap_err()
                .kind(),
            PlanValidationErrorKind::SourceChanged { .. }
        ));
        assert_eq!(fs::read(&source).unwrap(), vec![7_u8; 16]);
        assert!(fs::read_dir(&destination).unwrap().next().is_none());
    }
}
