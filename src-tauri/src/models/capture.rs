//! Capture request/progress models.

use super::common::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureRequest {
    /// Destination root chosen by the technician; the bundle is created at
    /// `<destination_root>/migrations/<name>-<timestamp>-<short-id>/`.
    pub destination_root: String,
    /// Discovery item ids. Unknown ids are rejected.
    pub selected_item_ids: Vec<String>,
    pub encryption: EncryptionRequest,
    pub options: CaptureOptions,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EncryptionRequest {
    pub enabled: bool,
    /// Only present in the request; never logged or persisted.
    #[serde(default, skip_serializing)]
    pub passphrase: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureOptions {
    /// Skip files held open by other processes after retries instead of failing the task.
    pub skip_locked_files: bool,
    /// Include browser caches (off by default).
    pub include_browser_cache: bool,
    /// Re-read every written file and compare SHA-256 (on by default).
    pub verify_after_copy: bool,
    pub max_retries: u32,
}

impl Default for CaptureOptions {
    fn default() -> Self {
        Self { skip_locked_files: true, include_browser_cache: false, verify_after_copy: true, max_retries: 3 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Queued,
    Scanning,
    Copying,
    Verifying,
    Completed,
    CompletedWithWarnings,
    Skipped,
    Failed,
    Canceled,
}

impl TaskState {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            TaskState::Completed
                | TaskState::CompletedWithWarnings
                | TaskState::Skipped
                | TaskState::Failed
                | TaskState::Canceled
        )
    }
}

/// Per-task progress snapshot, emitted as the `capture://progress` and
/// `restore://progress` events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskProgress {
    pub task_id: String,
    pub category: Category,
    pub display_name: String,
    pub state: TaskState,
    pub current_path: Option<String>,
    pub bytes_done: u64,
    pub bytes_total: Option<u64>,
    pub items_done: u64,
    pub items_total: Option<u64>,
    pub bytes_per_second: Option<f64>,
    pub elapsed_ms: u64,
    /// `None` when not meaningful (too early, unknown totals).
    pub eta_seconds: Option<u64>,
    pub warning_count: u32,
    pub retry_count: u32,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub level: Severity,
    pub task_id: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreflightReport {
    pub destination_root: String,
    pub destination_writable: bool,
    pub destination_free_bytes: Option<u64>,
    pub source_free_bytes: Option<u64>,
    pub estimated_bytes: u64,
    pub unknown_size_items: usize,
    pub sufficient_space: bool,
    pub long_paths_enabled: Option<bool>,
    pub destination_file_system: Option<String>,
    pub selected_count: usize,
    pub running_apps: Vec<String>,
    pub warnings: Vec<Warning>,
    pub blocking_errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureSummary {
    pub bundle_id: String,
    pub bundle_path: String,
    pub machine_name: String,
    pub captured_users: Vec<String>,
    pub total_bytes: u64,
    pub total_files: u64,
    pub verified: bool,
    pub status: super::manifest::BundleStatus,
    pub warnings: Vec<Warning>,
    pub task_results: Vec<TaskProgress>,
    pub report_html: String,
    pub report_json: String,
    pub summary_report_html: String,
}
