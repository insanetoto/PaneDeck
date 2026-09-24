use std::path::Path;

use crate::{
    OperationKind, OperationPlan, OperationPlanner, PlanValidationErrorKind, PreflightProbe,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrashBackendError {
    PermissionDenied,
    NotFound,
    Unsupported,
    Platform,
}

/// Injectable system-trash boundary. It deliberately has no permanent-delete API.
pub trait TrashBackend {
    fn move_to_trash(&self, path: &Path) -> Result<(), TrashBackendError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrashErrorCode {
    InvalidPlan,
    SourceChanged,
    PermissionDenied,
    Unsupported,
    Platform,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrashItemOutcome {
    Trashed,
    Failed(TrashErrorCode),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TrashItemResult {
    pub source_index: usize,
    pub outcome: TrashItemOutcome,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrashBatchResult {
    pub items: Vec<TrashItemResult>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct TrashExecutor;

impl TrashExecutor {
    pub fn execute<P: PreflightProbe, B: TrashBackend>(
        &self,
        plan: &OperationPlan,
        probe: &P,
        backend: &B,
    ) -> TrashBatchResult {
        if plan.kind() != OperationKind::Trash {
            return failed_for_all(plan.sources().len(), TrashErrorCode::InvalidPlan);
        }
        if let Err(error) = OperationPlanner.revalidate(plan, probe) {
            return failed_for_all(plan.sources().len(), map_validation_error(error.kind()));
        }

        TrashBatchResult {
            items: plan
                .sources()
                .iter()
                .enumerate()
                .map(|(source_index, source)| {
                    let outcome = match backend.move_to_trash(source.native_path().as_path()) {
                        Ok(()) => TrashItemOutcome::Trashed,
                        Err(error) => TrashItemOutcome::Failed(map_backend_error(error)),
                    };
                    TrashItemResult {
                        source_index,
                        outcome,
                    }
                })
                .collect(),
        }
    }
}

fn failed_for_all(count: usize, code: TrashErrorCode) -> TrashBatchResult {
    TrashBatchResult {
        items: (0..count)
            .map(|source_index| TrashItemResult {
                source_index,
                outcome: TrashItemOutcome::Failed(code),
            })
            .collect(),
    }
}

fn map_validation_error(kind: &PlanValidationErrorKind) -> TrashErrorCode {
    match kind {
        PlanValidationErrorKind::MissingSource { .. }
        | PlanValidationErrorKind::SourceChanged { .. } => TrashErrorCode::SourceChanged,
        PlanValidationErrorKind::ParentNotWritable => TrashErrorCode::PermissionDenied,
        _ => TrashErrorCode::InvalidPlan,
    }
}

const fn map_backend_error(error: TrashBackendError) -> TrashErrorCode {
    match error {
        TrashBackendError::PermissionDenied => TrashErrorCode::PermissionDenied,
        TrashBackendError::NotFound => TrashErrorCode::SourceChanged,
        TrashBackendError::Unsupported => TrashErrorCode::Unsupported,
        TrashBackendError::Platform => TrashErrorCode::Platform,
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, fs};

    use panedeck_domain::NativePath;
    use tempfile::TempDir;

    use crate::{OperationRequest, StandardPreflightProbe};

    use super::*;

    struct MockTrash {
        calls: Cell<usize>,
        fail_at: Option<usize>,
        error: TrashBackendError,
    }

    impl TrashBackend for MockTrash {
        fn move_to_trash(&self, _path: &Path) -> Result<(), TrashBackendError> {
            let index = self.calls.get();
            self.calls.set(index + 1);
            if self.fail_at == Some(index) {
                Err(self.error)
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn returns_a_result_for_every_item_and_maps_permission_errors() {
        let temp = TempDir::new().expect("temp dir");
        let sources: Vec<_> = (0..3)
            .map(|index| {
                let path = temp.path().join(format!("item-{index}"));
                fs::write(&path, b"data").expect("source");
                NativePath::new(path)
            })
            .collect();
        let plan = OperationPlanner
            .plan(OperationRequest::Trash { sources }, &StandardPreflightProbe)
            .expect("trash plan");
        let backend = MockTrash {
            calls: Cell::new(0),
            fail_at: Some(1),
            error: TrashBackendError::PermissionDenied,
        };

        let result = TrashExecutor.execute(&plan, &StandardPreflightProbe, &backend);
        assert_eq!(result.items.len(), 3);
        assert_eq!(result.items[0].outcome, TrashItemOutcome::Trashed);
        assert_eq!(
            result.items[1].outcome,
            TrashItemOutcome::Failed(TrashErrorCode::PermissionDenied)
        );
        assert_eq!(result.items[2].outcome, TrashItemOutcome::Trashed);
        assert_eq!(backend.calls.get(), 3);
    }

    #[test]
    fn changed_source_prevents_any_backend_call() {
        let temp = TempDir::new().expect("temp dir");
        let source = temp.path().join("item");
        fs::write(&source, b"data").expect("source");
        let plan = OperationPlanner
            .plan(
                OperationRequest::Trash {
                    sources: vec![NativePath::new(&source)],
                },
                &StandardPreflightProbe,
            )
            .expect("trash plan");
        fs::remove_file(&source).expect("remove test-owned source");
        let backend = MockTrash {
            calls: Cell::new(0),
            fail_at: None,
            error: TrashBackendError::Platform,
        };

        let result = TrashExecutor.execute(&plan, &StandardPreflightProbe, &backend);
        assert_eq!(
            result.items[0].outcome,
            TrashItemOutcome::Failed(TrashErrorCode::SourceChanged)
        );
        assert_eq!(backend.calls.get(), 0);
    }
}
