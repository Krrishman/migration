//! Progress reporting and structured logging shared by capture and restore.
//!
//! A [`ProgressSink`] receives task snapshots and log entries; the Tauri
//! layer forwards them to the UI as events, tests collect them in memory.
//! Log messages are built only from paths, counts and error kinds — the
//! passphrase and other secrets never flow through this module.

use crate::models::*;
use crate::util::RateEstimator;
use parking_lot::Mutex;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub trait ProgressSink: Send + Sync {
    fn task(&self, progress: &TaskProgress);
    fn log(&self, entry: &LogEntry);
}

/// Sink that discards everything.
pub struct NullSink;
impl ProgressSink for NullSink {
    fn task(&self, _: &TaskProgress) {}
    fn log(&self, _: &LogEntry) {}
}

/// Sink that records events in memory (tests, CLI).
#[derive(Default)]
pub struct MemorySink {
    pub tasks: Mutex<Vec<TaskProgress>>,
    pub logs: Mutex<Vec<LogEntry>>,
}
impl ProgressSink for MemorySink {
    fn task(&self, p: &TaskProgress) {
        self.tasks.lock().push(p.clone());
    }
    fn log(&self, e: &LogEntry) {
        self.logs.lock().push(e.clone());
    }
}

/// JSON-lines log file plus counters, forwarding to a sink.
pub struct Logger {
    file: Mutex<Option<std::fs::File>>,
    path: PathBuf,
    sink: Arc<dyn ProgressSink>,
    counts: Mutex<(u64, u64, u64)>,
    errors: Mutex<Vec<String>>,
}

impl Logger {
    pub fn new(path: &Path, sink: Arc<dyn ProgressSink>) -> Self {
        let file = std::fs::OpenOptions::new().create(true).append(true).open(path).ok();
        Self { file: Mutex::new(file), path: path.to_path_buf(), sink, counts: Mutex::new((0, 0, 0)), errors: Mutex::new(Vec::new()) }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn log(&self, level: Severity, task_id: Option<&str>, message: impl Into<String>) {
        let entry = LogEntry { timestamp: chrono::Utc::now(), level, task_id: task_id.map(str::to_string), message: message.into() };
        {
            let mut c = self.counts.lock();
            match level {
                Severity::Info => c.0 += 1,
                Severity::Warning => c.1 += 1,
                Severity::Error => {
                    c.2 += 1;
                    let mut errs = self.errors.lock();
                    if errs.len() < 200 {
                        errs.push(entry.message.clone());
                    }
                }
            }
        }
        if let Some(f) = self.file.lock().as_mut() {
            if let Ok(line) = serde_json::to_string(&entry) {
                let _ = writeln!(f, "{line}");
            }
        }
        self.sink.log(&entry);
    }

    pub fn info(&self, task: Option<&str>, msg: impl Into<String>) {
        self.log(Severity::Info, task, msg);
    }
    pub fn warn(&self, task: Option<&str>, msg: impl Into<String>) {
        self.log(Severity::Warning, task, msg);
    }
    pub fn error(&self, task: Option<&str>, msg: impl Into<String>) {
        self.log(Severity::Error, task, msg);
    }

    pub fn summary(&self, rel_path: &str) -> LogSummary {
        let c = *self.counts.lock();
        LogSummary { log_file: rel_path.into(), info_count: c.0, warning_count: c.1, error_count: c.2, errors: self.errors.lock().clone() }
    }

    pub fn flush(&self) {
        if let Some(f) = self.file.lock().as_mut() {
            let _ = f.flush();
        }
    }
}

/// Tracks one task's progress and emits throttled snapshots.
pub struct TaskTracker {
    pub progress: TaskProgress,
    rate: RateEstimator,
    started: Instant,
    last_emit: Option<Instant>,
    sink: Arc<dyn ProgressSink>,
}

impl TaskTracker {
    pub fn new(task_id: &str, category: Category, display_name: &str, sink: Arc<dyn ProgressSink>) -> Self {
        let progress = TaskProgress {
            task_id: task_id.into(),
            category,
            display_name: display_name.into(),
            state: TaskState::Queued,
            current_path: None,
            bytes_done: 0,
            bytes_total: None,
            items_done: 0,
            items_total: None,
            bytes_per_second: None,
            elapsed_ms: 0,
            eta_seconds: None,
            warning_count: 0,
            retry_count: 0,
            error: None,
        };
        Self { progress, rate: RateEstimator::new(), started: Instant::now(), last_emit: None, sink }
    }

    pub fn start(&mut self) {
        self.started = Instant::now();
        self.rate = RateEstimator::starting_at(self.started);
    }

    pub fn set_state(&mut self, state: TaskState) {
        self.progress.state = state;
        self.emit(true);
    }

    pub fn set_totals(&mut self, bytes: Option<u64>, items: Option<u64>) {
        self.progress.bytes_total = bytes;
        self.progress.items_total = items;
        self.emit(true);
    }

    pub fn advance(&mut self, bytes: u64, items: u64, current: Option<&str>) {
        self.progress.bytes_done += bytes;
        self.progress.items_done += items;
        if let Some(c) = current {
            self.progress.current_path = Some(c.to_string());
        }
        self.emit(false);
    }

    pub fn warn(&mut self) {
        self.progress.warning_count += 1;
    }

    pub fn retried(&mut self) {
        self.progress.retry_count += 1;
    }

    pub fn emit(&mut self, force: bool) {
        let now = Instant::now();
        if !force && self.last_emit.is_some_and(|t| now.duration_since(t) < Duration::from_millis(150)) {
            return;
        }
        self.rate.sample(self.progress.bytes_done, now);
        self.progress.elapsed_ms = now.duration_since(self.started).as_millis() as u64;
        self.progress.bytes_per_second = self.rate.rate();
        self.progress.eta_seconds = if self.progress.state.is_terminal() {
            None
        } else {
            self.rate.eta_seconds(self.progress.bytes_done, self.progress.bytes_total, now)
        };
        self.last_emit = Some(now);
        self.sink.task(&self.progress);
    }
}
