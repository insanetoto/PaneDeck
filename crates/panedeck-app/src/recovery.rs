use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecoveryError;

impl std::fmt::Display for RecoveryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("recovery cleanup failed")
    }
}

impl std::error::Error for RecoveryError {}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct JournalEvent {
    run_id: String,
    job_id: u64,
    kind: String,
    state: String,
    checkpoint: String,
    timestamp_millis: u64,
    source_paths: Vec<String>,
    destination_directories: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryItemDto {
    pub recovery_id: String,
    pub job_id: u64,
    pub kind: String,
    pub checkpoint: String,
    pub source_items_present: usize,
    pub residual_items: usize,
    pub cleanup_available: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CleanupRecoveryRequestDto {
    pub recovery_id: String,
}

#[derive(Clone, Debug)]
struct RecoveryItem {
    dto: RecoveryItemDto,
    residual_paths: Vec<PathBuf>,
}

#[derive(Debug)]
struct RecoveryInner {
    journal_path: Option<PathBuf>,
    run_id: String,
    recovered: BTreeMap<String, RecoveryItem>,
}

#[derive(Clone, Debug)]
pub struct RecoveryService {
    inner: Arc<Mutex<RecoveryInner>>,
}

impl Default for RecoveryService {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(RecoveryInner {
                journal_path: None,
                run_id: run_id(),
                recovered: BTreeMap::new(),
            })),
        }
    }
}

impl RecoveryService {
    #[must_use]
    pub fn new(journal_path: PathBuf) -> Self {
        if let Some(parent) = journal_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let current_run = run_id();
        let recovered = scan_journal(&journal_path, &current_run);
        Self {
            inner: Arc::new(Mutex::new(RecoveryInner {
                journal_path: Some(journal_path),
                run_id: current_run,
                recovered,
            })),
        }
    }

    pub(crate) fn record(
        &self,
        job_id: u64,
        kind: &str,
        state: &str,
        checkpoint: &str,
        source_paths: &[PathBuf],
        destination_directories: &[PathBuf],
    ) {
        let inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(path) = &inner.journal_path else {
            return;
        };
        let event = JournalEvent {
            run_id: inner.run_id.clone(),
            job_id,
            kind: kind.to_owned(),
            state: state.to_owned(),
            checkpoint: checkpoint.to_owned(),
            timestamp_millis: now_millis(),
            source_paths: source_paths
                .iter()
                .map(|value| value.to_string_lossy().into_owned())
                .collect(),
            destination_directories: destination_directories
                .iter()
                .map(|value| value.to_string_lossy().into_owned())
                .collect(),
        };
        let Ok(serialized) = serde_json::to_string(&event) else {
            return;
        };
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(file, "{serialized}");
            let _ = file.sync_data();
        }
    }

    #[must_use]
    pub fn items(&self) -> Vec<RecoveryItemDto> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .recovered
            .values()
            .map(|item| item.dto.clone())
            .collect()
    }

    pub fn confirm_cleanup(&self, recovery_id: &str) -> Result<(), RecoveryError> {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let item = inner.recovered.get(recovery_id).ok_or(RecoveryError)?;
        for path in &item.residual_paths {
            if !is_safe_partial(path) {
                return Err(RecoveryError);
            }
            let metadata = match fs::symlink_metadata(path) {
                Ok(value) => value,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(_) => return Err(RecoveryError),
            };
            let result = if metadata.is_dir() && !metadata.file_type().is_symlink() {
                fs::remove_dir_all(path)
            } else {
                fs::remove_file(path)
            };
            if result.is_err() {
                return Err(RecoveryError);
            }
        }
        inner.recovered.remove(recovery_id);
        Ok(())
    }

    #[must_use]
    pub fn diagnostic_events(&self) -> Vec<serde_json::Value> {
        let inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(path) = &inner.journal_path else {
            return Vec::new();
        };
        read_events(path)
            .into_iter()
            .rev()
            .take(200)
            .map(|event| {
                serde_json::json!({
                    "runId": anonymize(&event.run_id),
                    "jobId": event.job_id,
                    "kind": event.kind,
                    "state": event.state,
                    "checkpoint": event.checkpoint,
                    "timestampMillis": event.timestamp_millis,
                    "sourceCount": event.source_paths.len(),
                    "destinationCount": event.destination_directories.len()
                })
            })
            .collect()
    }
}

fn scan_journal(path: &Path, current_run: &str) -> BTreeMap<String, RecoveryItem> {
    let mut latest = BTreeMap::<(String, u64), JournalEvent>::new();
    for event in read_events(path) {
        latest.insert((event.run_id.clone(), event.job_id), event);
    }
    latest
        .into_values()
        .filter(|event| event.run_id != current_run && !is_terminal(&event.state))
        .map(|event| {
            let recovery_id = format!("{}:{}", event.run_id, event.job_id);
            let residual_paths = event
                .destination_directories
                .iter()
                .flat_map(|directory| find_partials(Path::new(directory)))
                .collect::<Vec<_>>();
            let source_items_present = event
                .source_paths
                .iter()
                .filter(|source| fs::symlink_metadata(source).is_ok())
                .count();
            let dto = RecoveryItemDto {
                recovery_id: recovery_id.clone(),
                job_id: event.job_id,
                kind: event.kind,
                checkpoint: event.checkpoint,
                source_items_present,
                residual_items: residual_paths.len(),
                cleanup_available: residual_paths.iter().all(|path| is_safe_partial(path)),
            };
            (
                recovery_id,
                RecoveryItem {
                    dto,
                    residual_paths,
                },
            )
        })
        .collect()
}

fn read_events(path: &Path) -> Vec<JournalEvent> {
    let Ok(file) = fs::File::open(path) else {
        return Vec::new();
    };
    BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| serde_json::from_str(&line).ok())
        .collect()
}

fn find_partials(directory: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| is_owned_residue(path))
        .collect()
}

fn is_safe_partial(path: &Path) -> bool {
    path.file_name().is_some_and(|name| {
        let name = name.to_string_lossy();
        name.starts_with(".panedeck-copy-") && name.ends_with(".partial")
    })
}

fn is_owned_residue(path: &Path) -> bool {
    is_safe_partial(path)
        || path.file_name().is_some_and(|name| {
            let name = name.to_string_lossy();
            name.starts_with(".panedeck-replaced-") && name.ends_with(".backup")
        })
}

fn is_terminal(state: &str) -> bool {
    matches!(
        state,
        "completed" | "partiallyFailed" | "failed" | "cancelled"
    )
}

fn run_id() -> String {
    static NEXT_RUN: AtomicU64 = AtomicU64::new(1);
    format!(
        "{}-{}-{}",
        now_millis(),
        std::process::id(),
        NEXT_RUN.fetch_add(1, Ordering::Relaxed)
    )
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis() as u64)
}

fn anonymize(value: &str) -> String {
    let hash = value.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        hash.wrapping_mul(0x100_0000_01b3) ^ u64::from(byte)
    });
    format!("run-{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn interrupted_job_is_recovered_without_deleting_source() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("source-secret.txt");
        let destination = temp.path().join("destination");
        fs::write(&source, b"keep me").unwrap();
        fs::create_dir(&destination).unwrap();
        fs::write(destination.join(".panedeck-copy-9-1.partial"), b"partial").unwrap();
        let journal_path = temp.path().join("journal.jsonl");
        let first = RecoveryService::new(journal_path.clone());
        first.record(
            7,
            "move",
            "running",
            "copying",
            std::slice::from_ref(&source),
            std::slice::from_ref(&destination),
        );
        drop(first);

        let restarted = RecoveryService::new(journal_path);
        let item = restarted.items().pop().expect("interrupted task");
        assert_eq!(item.source_items_present, 1);
        assert_eq!(item.residual_items, 1);
        assert_eq!(fs::read(&source).unwrap(), b"keep me");
        restarted.confirm_cleanup(&item.recovery_id).unwrap();
        assert!(source.exists());
        assert!(!destination.join(".panedeck-copy-9-1.partial").exists());
    }

    #[test]
    fn diagnostic_events_never_include_paths_or_file_contents() {
        let temp = TempDir::new().unwrap();
        let journal_path = temp.path().join("journal.jsonl");
        let service = RecoveryService::new(journal_path);
        let secret = temp.path().join("private-file-name.txt");
        fs::write(&secret, b"very private contents").unwrap();
        service.record(
            1,
            "copy",
            "running",
            "copying",
            &[secret],
            &[temp.path().into()],
        );
        let json = serde_json::to_string(&service.diagnostic_events()).unwrap();
        assert!(!json.contains(temp.path().to_string_lossy().as_ref()));
        assert!(!json.contains("private-file-name"));
        assert!(!json.contains("very private contents"));
    }

    #[test]
    fn startup_marks_only_nonterminal_checkpoints_as_interrupted() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("source.txt");
        let destination = temp.path().join("destination");
        fs::write(&source, b"source stays").unwrap();
        fs::create_dir(&destination).unwrap();
        let journal = temp.path().join("journal.jsonl");
        let first = RecoveryService::new(journal.clone());
        for (job_id, state, checkpoint) in [
            (1, "queued", "planned"),
            (2, "running", "executing"),
            (3, "completed", "commit-complete"),
        ] {
            first.record(
                job_id,
                "move",
                state,
                checkpoint,
                std::slice::from_ref(&source),
                std::slice::from_ref(&destination),
            );
        }
        drop(first);

        let restarted = RecoveryService::new(journal);
        let recovered = restarted.items();
        assert_eq!(recovered.len(), 2);
        assert!(recovered.iter().all(|item| item.source_items_present == 1));
        assert_eq!(fs::read(source).unwrap(), b"source stays");
    }
}
