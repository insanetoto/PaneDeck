use crate::{OperationItemOutcomeDto, OperationJobStateDto, OperationKindDto};
use serde::Serialize;
use std::{
    collections::{BTreeMap, VecDeque},
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{SystemTime, UNIX_EPOCH},
};

pub const LOG_FILE_BYTES: u64 = 10 * 1024 * 1024;
pub const LOG_FILE_COUNT: usize = 5;
const RECENT_LIMIT: usize = 1000;
static NEXT_ID: AtomicU64 = AtomicU64::new(1);
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogStatusDto {
    pub persistence_enabled: bool,
    pub failed_writes: u64,
    pub last_error: Option<String>,
}
#[derive(Clone, Debug, serde::Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationLogEventDto {
    pub schema_version: u8,
    pub operation_id: String,
    pub job_id: Option<u64>,
    pub timestamp_millis: u64,
    pub kind: OperationKindDto,
    pub event: String,
    pub state: Option<OperationJobStateDto>,
    pub outcomes: Vec<OperationItemOutcomeDto>,
    pub errors: Vec<ItemErrorDto>,
}
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemErrorDto {
    pub index: Option<usize>,
    pub domain: String,
    pub code: String,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationLogSnapshotDto {
    pub events: Vec<OperationLogEventDto>,
    pub status: LogStatusDto,
    pub recovery_status: LogStatusDto,
}
struct Inner {
    path: Option<PathBuf>,
    status: LogStatusDto,
    recent: VecDeque<OperationLogEventDto>,
    max_bytes: u64,
    finished: BTreeMap<String, (OperationJobStateDto, Vec<OperationItemOutcomeDto>)>,
    #[cfg(test)]
    fail_sync: bool,
}
#[derive(Clone)]
pub struct OperationLog {
    inner: Arc<Mutex<Inner>>,
}
impl Default for OperationLog {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                path: None,
                status: LogStatusDto::default(),
                recent: VecDeque::new(),
                max_bytes: LOG_FILE_BYTES,
                finished: BTreeMap::new(),
                #[cfg(test)]
                fail_sync: false,
            })),
        }
    }
}
impl OperationLog {
    pub fn new(path: PathBuf) -> Self {
        let log = Self::default();
        {
            let mut inner = log.inner.lock().unwrap();
            inner.status.persistence_enabled = true;
            for index in (0..LOG_FILE_COUNT).rev() {
                let file = if index == 0 {
                    path.clone()
                } else {
                    path.with_extension(format!("jsonl.{index}"))
                };
                if let Ok(file) = fs::File::open(file) {
                    for line in BufReader::new(file).lines().map_while(Result::ok) {
                        if let Ok(event) = serde_json::from_str(&line) {
                            inner.recent.push_back(event);
                            if inner.recent.len() > RECENT_LIMIT {
                                inner.recent.pop_front();
                            }
                        }
                    }
                }
            }
            inner.path = Some(path);
        }
        log
    }
    pub fn begin(&self, kind: OperationKindDto) -> String {
        let id = format!(
            "{}-{}-{}",
            now(),
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        );
        self.record(&id, None, kind, "requested", None, &[], &[]);
        id
    }
    #[allow(clippy::too_many_arguments)]
    pub fn record(
        &self,
        id: &str,
        job: Option<u64>,
        kind: OperationKindDto,
        event: &str,
        state: Option<OperationJobStateDto>,
        outcomes: &[OperationItemOutcomeDto],
        errors: &[ItemErrorDto],
    ) {
        let event = OperationLogEventDto {
            schema_version: 1,
            operation_id: id.into(),
            job_id: job,
            timestamp_millis: now(),
            kind,
            event: event.into(),
            state,
            outcomes: outcomes.into(),
            errors: errors.into(),
        };
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if matches!(event.event.as_str(), "finished" | "rejected") {
            if let Some(state) = event.state {
                inner
                    .finished
                    .insert(id.into(), (state, event.outcomes.clone()));
            }
        }
        inner.recent.push_back(event.clone());
        if inner.recent.len() > RECENT_LIMIT {
            inner.recent.pop_front();
        }
        if let Some(path) = inner.path.clone() {
            let result = (|| -> std::io::Result<()> {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)?;
                }
                let bytes = serde_json::to_vec(&event)?;
                if bytes.len() as u64 + 1 > inner.max_bytes {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "operation log event exceeds file limit",
                    ));
                }
                if fs::metadata(&path).map(|m| m.len()).unwrap_or(0) + bytes.len() as u64 + 1
                    > inner.max_bytes
                {
                    let oldest = path.with_extension(format!("jsonl.{}", LOG_FILE_COUNT - 1));
                    match fs::remove_file(oldest) {
                        Ok(()) => {}
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                        Err(e) => return Err(e),
                    }
                    for index in (1..LOG_FILE_COUNT - 1).rev() {
                        let old = path.with_extension(format!("jsonl.{index}"));
                        if old.exists() {
                            fs::rename(old, path.with_extension(format!("jsonl.{}", index + 1)))?;
                        }
                    }
                    if path.exists() {
                        fs::rename(&path, path.with_extension("jsonl.1"))?;
                    }
                }
                let mut file = OpenOptions::new().create(true).append(true).open(path)?;
                file.write_all(&bytes)?;
                file.write_all(b"\n")?;
                #[cfg(test)]
                if inner.fail_sync {
                    return Err(std::io::Error::other("injected sync failure"));
                }
                file.sync_data()
            })();
            if let Err(e) = result {
                inner.status.failed_writes += 1;
                inner.status.last_error = Some(format!("{:?}", e.kind()));
            }
        }
    }
    // Live job summaries outlive disk rotation and the bounded log preview.
    pub fn result(&self, id: &str) -> Option<(OperationJobStateDto, Vec<OperationItemOutcomeDto>)> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .finished
            .get(id)
            .cloned()
    }
    pub fn status(&self) -> LogStatusDto {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .status
            .clone()
    }
    pub fn recent(&self) -> Vec<OperationLogEventDto> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .recent
            .iter()
            .cloned()
            .collect()
    }
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rotation_keeps_five_files_and_leaves_recovery_evidence() {
        let temp = tempfile::TempDir::new().unwrap();
        let path = temp.path().join("operations.jsonl");
        fs::write(temp.path().join("recovery.jsonl"), b"unfinished evidence").unwrap();
        let log = OperationLog::new(path.clone());
        log.inner.lock().unwrap().max_bytes = 400;
        for _ in 0..20 {
            log.begin(OperationKindDto::Copy);
        }
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 6);
        assert_eq!(
            fs::read(temp.path().join("recovery.jsonl")).unwrap(),
            b"unfinished evidence"
        );
        assert_eq!(log.status().failed_writes, 0);
        assert!(!OperationLog::new(path).recent().is_empty());
    }
    #[test]
    fn recent_log_eviction_preserves_live_logical_results_and_file_limits() {
        let temp = tempfile::TempDir::new().unwrap();
        let path = temp.path().join("operations.jsonl");
        let log = OperationLog::new(path.clone());
        let id = log.begin(OperationKindDto::Copy);
        log.record(
            &id,
            None,
            OperationKindDto::Copy,
            "finished",
            Some(OperationJobStateDto::Completed),
            &[OperationItemOutcomeDto::Succeeded],
            &[],
        );
        for _ in 0..RECENT_LIMIT {
            log.begin(OperationKindDto::Copy);
        }
        assert!(log.result(&id).is_some());
        assert!(!log.recent().iter().any(|e| e.operation_id == id));
        log.inner.lock().unwrap().max_bytes = 10;
        log.begin(OperationKindDto::Copy);
        assert_eq!(log.status().failed_writes, 1);
    }

    #[test]
    fn sync_failure_is_not_reported_as_saved() {
        let temp = tempfile::TempDir::new().unwrap();
        let log = OperationLog::new(temp.path().join("operations.jsonl"));
        log.inner.lock().unwrap().fail_sync = true;
        log.begin(OperationKindDto::Copy);
        assert_eq!(log.status().failed_writes, 1);
        assert_eq!(log.status().last_error.as_deref(), Some("Other"));
    }
    #[test]
    fn failure_is_visible_and_memory_diagnostics_survive() {
        let temp = tempfile::TempDir::new().unwrap();
        let parent = temp.path().join("blocked");
        fs::write(&parent, b"file").unwrap();
        let log = OperationLog::new(parent.join("operations.jsonl"));
        log.begin(OperationKindDto::Move);
        assert_eq!(log.status().failed_writes, 1);
        assert!(log.status().last_error.is_some());
        assert_eq!(log.recent().len(), 1);
    }
}
