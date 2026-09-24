use std::{fs, io};

use crate::{
    OperationKind, OperationPlan, OperationPlanner, PlanValidationErrorKind, PreflightProbe,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MutationErrorCode {
    InvalidName,
    InvalidPlan,
    SourceChanged,
    TargetConflict,
    PermissionDenied,
    Io,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct MutationExecutor;

impl MutationExecutor {
    pub fn execute<P: PreflightProbe>(
        &self,
        plan: &OperationPlan,
        probe: &P,
    ) -> Result<(), MutationErrorCode> {
        OperationPlanner
            .revalidate(plan, probe)
            .map_err(|error| map_validation_error(error.kind()))?;
        match plan.kind() {
            OperationKind::Rename if plan.sources().len() == 1 && plan.targets().len() == 1 => {
                fs::rename(
                    plan.sources()[0].native_path().as_path(),
                    plan.targets()[0].native_path().as_path(),
                )
                .map_err(map_io_error)
            }
            OperationKind::CreateDirectory
                if plan.sources().is_empty() && plan.targets().len() == 1 =>
            {
                fs::create_dir(plan.targets()[0].native_path().as_path()).map_err(map_io_error)
            }
            _ => Err(MutationErrorCode::InvalidPlan),
        }
    }
}

fn map_validation_error(kind: &PlanValidationErrorKind) -> MutationErrorCode {
    match kind {
        PlanValidationErrorKind::InvalidName => MutationErrorCode::InvalidName,
        PlanValidationErrorKind::Conflict { .. }
        | PlanValidationErrorKind::TargetChanged { .. } => MutationErrorCode::TargetConflict,
        PlanValidationErrorKind::MissingSource { .. }
        | PlanValidationErrorKind::SourceChanged { .. } => MutationErrorCode::SourceChanged,
        PlanValidationErrorKind::ParentNotWritable
        | PlanValidationErrorKind::SourceNotReadable { .. } => MutationErrorCode::PermissionDenied,
        _ => MutationErrorCode::InvalidPlan,
    }
}

fn map_io_error(error: io::Error) -> MutationErrorCode {
    match error.kind() {
        io::ErrorKind::AlreadyExists => MutationErrorCode::TargetConflict,
        io::ErrorKind::NotFound => MutationErrorCode::SourceChanged,
        io::ErrorKind::PermissionDenied => MutationErrorCode::PermissionDenied,
        io::ErrorKind::InvalidInput | io::ErrorKind::InvalidData => MutationErrorCode::InvalidName,
        _ => MutationErrorCode::Io,
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use panedeck_domain::NativePath;
    use tempfile::TempDir;

    use crate::{OperationRequest, StandardPreflightProbe};

    use super::*;

    #[test]
    fn renames_an_item_and_creates_a_directory() {
        let temp = TempDir::new().expect("temp dir");
        let source = temp.path().join("before.txt");
        fs::write(&source, b"payload").expect("source");
        let rename = OperationPlanner
            .plan(
                OperationRequest::Rename {
                    source: NativePath::new(&source),
                    new_name: "after.txt".into(),
                },
                &StandardPreflightProbe,
            )
            .expect("rename plan");
        MutationExecutor
            .execute(&rename, &StandardPreflightProbe)
            .expect("rename");
        assert_eq!(
            fs::read(temp.path().join("after.txt")).expect("target"),
            b"payload"
        );

        let create = OperationPlanner
            .plan(
                OperationRequest::CreateDirectory {
                    parent: NativePath::new(temp.path()),
                    name: "folder".into(),
                },
                &StandardPreflightProbe,
            )
            .expect("create plan");
        MutationExecutor
            .execute(&create, &StandardPreflightProbe)
            .expect("create");
        assert!(temp.path().join("folder").is_dir());
    }

    #[test]
    fn rejects_empty_separator_and_existing_names_without_writing() {
        let temp = TempDir::new().expect("temp dir");
        let source = temp.path().join("source.txt");
        fs::write(&source, b"source").expect("source");
        fs::write(temp.path().join("existing.txt"), b"existing").expect("existing");

        for name in [OsString::from(""), OsString::from("nested/name")] {
            let result = OperationPlanner.plan(
                OperationRequest::Rename {
                    source: NativePath::new(&source),
                    new_name: name,
                },
                &StandardPreflightProbe,
            );
            assert!(matches!(
                result.expect_err("invalid name").kind(),
                PlanValidationErrorKind::InvalidName
            ));
        }
        let conflict = OperationPlanner.plan(
            OperationRequest::Rename {
                source: NativePath::new(&source),
                new_name: "existing.txt".into(),
            },
            &StandardPreflightProbe,
        );
        assert!(matches!(
            conflict.expect_err("conflict").kind(),
            PlanValidationErrorKind::Conflict { .. }
        ));
        assert_eq!(fs::read(&source).expect("source remains"), b"source");
    }

    #[test]
    fn supports_case_only_rename() {
        let temp = TempDir::new().expect("temp dir");
        let source = temp.path().join("report.txt");
        let target = temp.path().join("Report.txt");
        fs::write(&source, b"payload").expect("source");
        let plan = OperationPlanner
            .plan(
                OperationRequest::Rename {
                    source: NativePath::new(&source),
                    new_name: "Report.txt".into(),
                },
                &StandardPreflightProbe,
            )
            .expect("case-only plan");
        MutationExecutor
            .execute(&plan, &StandardPreflightProbe)
            .expect("case-only rename");
        assert_eq!(fs::read(target).expect("renamed target"), b"payload");
    }

    #[test]
    fn target_occupied_after_planning_is_reported_without_overwrite() {
        let temp = TempDir::new().expect("temp dir");
        let source = temp.path().join("source.txt");
        let target = temp.path().join("target.txt");
        fs::write(&source, b"source").expect("source");
        let plan = OperationPlanner
            .plan(
                OperationRequest::Rename {
                    source: NativePath::new(&source),
                    new_name: "target.txt".into(),
                },
                &StandardPreflightProbe,
            )
            .expect("rename plan");
        fs::write(&target, b"occupied").expect("occupied target");

        assert_eq!(
            MutationExecutor.execute(&plan, &StandardPreflightProbe),
            Err(MutationErrorCode::TargetConflict)
        );
        assert_eq!(fs::read(source).expect("source remains"), b"source");
        assert_eq!(fs::read(target).expect("target remains"), b"occupied");
    }
}
