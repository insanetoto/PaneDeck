use std::{fs::OpenOptions, io::Write, path::Path};

use serde::{Deserialize, Serialize};

use crate::{OperationJobDto, RecoveryService};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticPreviewDto {
    pub file_name: String,
    pub content: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiagnosticExportRequestDto {
    pub destination: String,
    pub content: String,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DiagnosticService;

impl DiagnosticService {
    pub fn preview(
        &self,
        jobs: &[OperationJobDto],
        recovery: &RecoveryService,
    ) -> DiagnosticPreviewDto {
        let jobs = jobs
            .iter()
            .map(|job| {
                serde_json::json!({
                    "jobId": job.job_id,
                    "kind": job.kind,
                    "state": job.state,
                    "itemCount": job.item_count,
                    "completedItems": job.completed_items,
                    "failedItems": job.failed_items,
                    "elapsedMillis": job.elapsed_millis,
                    "itemOutcomes": job.item_outcomes,
                })
            })
            .collect::<Vec<_>>();
        let value = serde_json::json!({
            "schemaVersion": 1,
            "product": "PaneDeck",
            "version": env!("CARGO_PKG_VERSION"),
            "platform": std::env::consts::OS,
            "architecture": std::env::consts::ARCH,
            "privacy": {
                "pathsRemoved": true,
                "fileNamesRemoved": true,
                "fileContentsIncluded": false,
                "automaticUpload": false,
            },
            "operationJobs": jobs,
            "operationLog": recovery.diagnostic_events(),
            "recoveryItemCount": recovery.items().len(),
        });
        DiagnosticPreviewDto {
            file_name: "panedeck-diagnostics.json".to_owned(),
            content: serde_json::to_string_pretty(&value).expect("diagnostic JSON is serializable"),
        }
    }

    pub fn export(&self, request: DiagnosticExportRequestDto) -> Result<(), String> {
        let destination = Path::new(&request.destination);
        if !destination.is_absolute() || request.content.is_empty() {
            return Err("invalid diagnostic destination or content".to_owned());
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)
            .map_err(|error| format!("could not create diagnostic export: {error}"))?;
        file.write_all(request.content.as_bytes())
            .and_then(|()| file.sync_all())
            .map_err(|error| format!("could not write diagnostic export: {error}"))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn preview_and_export_exclude_sensitive_paths_names_and_contents() {
        let temporary = TempDir::new().unwrap();
        let secret = temporary.path().join("secret-name.txt");
        fs::write(&secret, b"secret file contents").unwrap();
        let recovery = RecoveryService::new(temporary.path().join("journal.jsonl"));
        recovery.record(
            3,
            "copy",
            "running",
            "executing",
            std::slice::from_ref(&secret),
            &[temporary.path().to_owned()],
        );

        let preview = DiagnosticService.preview(&[], &recovery);
        assert!(!preview
            .content
            .contains(temporary.path().to_string_lossy().as_ref()));
        assert!(!preview.content.contains("secret-name"));
        assert!(!preview.content.contains("secret file contents"));
        let output = temporary.path().join("diagnostics.json");
        DiagnosticService
            .export(DiagnosticExportRequestDto {
                destination: output.to_string_lossy().into_owned(),
                content: preview.content.clone(),
            })
            .unwrap();
        assert_eq!(fs::read_to_string(output).unwrap(), preview.content);
    }
}
