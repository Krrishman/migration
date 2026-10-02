//! The versioned bundle manifest. This is the contract between capture and
//! restore and must never contain secrets (passwords, tokens, keys, cookies).

use super::common::*;
use super::discovery::*;
use serde::{Deserialize, Serialize};

/// Current manifest schema. Major version changes are breaking; readers reject
/// a different major version. Minor additions must be `#[serde(default)]`.
pub const SCHEMA_VERSION: &str = "1.0";
pub const SCHEMA_MAJOR: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub schema_version: String,
    pub bundle_id: String,
    pub app_version: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    #[serde(default)]
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub status: BundleStatus,
    pub source_machine: SourceMachine,
    /// Destination root as chosen at capture time (informational).
    pub destination_root: String,
    pub elevated: bool,
    pub encryption: EncryptionMetadata,
    pub users: Vec<ManifestUser>,
    pub selected_modules: Vec<String>,
    pub items: Vec<ManifestItem>,
    pub printers: Vec<PrinterInfo>,
    pub mapped_drives: Vec<MappedDrive>,
    pub applications: Vec<InstalledApp>,
    pub browser_profiles: Vec<BrowserProfileInfo>,
    pub restore_compatibility_notes: Vec<String>,
    pub log_summary: LogSummary,
    pub capacity: CapacitySnapshot,
    pub integrity: IntegrityMetadata,
    pub exclusions: Vec<Exclusion>,
    #[serde(default)]
    pub restore_history: Vec<RestoreEvent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BundleStatus {
    /// Capture started but not finished (crash, cancel). Resumable.
    InProgress,
    Canceled,
    /// All tasks finished; verification failed or tasks failed.
    CompletedUnverified,
    /// Every captured file was re-hashed and matched.
    Verified,
    VerifiedWithWarnings,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SourceMachine {
    pub computer_name: String,
    pub os_name: String,
    pub os_version: String,
    pub os_build: String,
    pub architecture: String,
    pub time_zone: String,
    pub join_state: JoinState,
    pub join_name: Option<String>,
    /// Always `None` in this version: no stable device ID is read. The field
    /// exists so the disclosure is explicit in the schema.
    pub device_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EncryptionMetadata {
    pub enabled: bool,
    /// "aes-256-gcm-stream-be32" when enabled.
    pub algorithm: Option<String>,
    pub kdf: Option<KdfParams>,
    /// Hex-encoded encryption of a fixed constant; used only to check a
    /// passphrase. It is not the key and does not reveal it.
    pub key_check: Option<String>,
    pub chunk_size: Option<u32>,
}

impl EncryptionMetadata {
    pub fn disabled() -> Self {
        Self { enabled: false, algorithm: None, kdf: None, key_check: None, chunk_size: None }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KdfParams {
    pub algorithm: String,
    /// Hex-encoded random salt (16 bytes).
    pub salt: String,
    pub memory_kib: u32,
    pub iterations: u32,
    pub parallelism: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ManifestUser {
    pub sid: String,
    pub account_name: String,
    pub display_name: Option<String>,
    pub profile_path: String,
    pub last_use: Option<chrono::DateTime<chrono::Utc>>,
    pub profile_size: Option<u64>,
    /// Bundle-relative folder that holds this user's payload (e.g. "users/jdoe").
    pub bundle_dir: String,
    pub selected_modules: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureStatus {
    Pending,
    Captured,
    CapturedWithWarnings,
    Skipped,
    Failed,
    Canceled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HashStatus {
    NotHashed,
    Hashed,
    Verified,
    Mismatch,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ManifestItem {
    pub id: String,
    pub category: Category,
    pub display_name: String,
    pub source_path: String,
    pub owner: Option<UserRef>,
    /// Path of the item relative to the owner's profile root, when it lives
    /// inside it. Restore maps this onto the destination profile.
    pub profile_relative: Option<String>,
    /// Bundle-relative directory or file holding the payload.
    pub bundle_path: String,
    pub restore_kind: RestoreKind,
    pub support: SupportLevel,
    pub estimated_size: Option<u64>,
    pub captured_bytes: u64,
    pub captured_files: u64,
    pub skipped_files: u64,
    pub hash_status: HashStatus,
    /// Bundle-relative path of the sha256sum-format hash list, if any.
    pub hash_list: Option<String>,
    /// SHA-256 of the hash list file (module-level root).
    pub hash_list_sha256: Option<String>,
    pub capture_status: CaptureStatus,
    pub warnings: Vec<Warning>,
    pub restore_notes: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct LogSummary {
    pub log_file: String,
    pub info_count: u64,
    pub warning_count: u64,
    pub error_count: u64,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct CapacitySnapshot {
    pub source_free_bytes: Option<u64>,
    pub source_total_bytes: Option<u64>,
    pub destination_free_bytes: Option<u64>,
    pub destination_total_bytes: Option<u64>,
    pub estimated_bundle_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IntegrityMetadata {
    /// Exactly which strategy was used, for auditability.
    pub strategy: String,
    pub hash_algorithm: String,
    pub verified: bool,
    pub verified_at: Option<chrono::DateTime<chrono::Utc>>,
    pub total_files: u64,
    pub total_bytes: u64,
    pub mismatches: u64,
    /// SHA-256 over the concatenated per-module hash-list digests, in item order.
    pub bundle_root_hash: Option<String>,
}

impl Default for IntegrityMetadata {
    fn default() -> Self {
        Self {
            strategy: crate::capture::hashing::HASH_STRATEGY.to_string(),
            hash_algorithm: "SHA-256".into(),
            verified: false,
            verified_at: None,
            total_files: 0,
            total_bytes: 0,
            mismatches: 0,
            bundle_root_hash: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Exclusion {
    pub pattern: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RestoreEvent {
    pub restore_id: String,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub finished_at: chrono::DateTime<chrono::Utc>,
    pub target_computer: String,
    pub app_version: String,
    pub user_mappings: Vec<(String, String)>,
    pub restored_items: Vec<String>,
    pub files_written: u64,
    pub files_skipped: u64,
    pub failures: u64,
    pub outcome: String,
    pub report_path: Option<String>,
}
