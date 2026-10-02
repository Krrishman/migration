//! Restore planning and execution models.

use super::common::*;
use super::discovery::UserProfile;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CollisionPolicy {
    /// Default. Existing destination files are left untouched.
    #[default]
    SkipExisting,
    /// Incoming file is written as "name (migrated).ext" / "name (migrated 2).ext".
    RenameIncoming,
    /// Existing file is first renamed to "name.pre-migration-<ts>.bak", then the
    /// incoming file is written. Requires per-category confirmation.
    ReplaceAfterConfirmation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserMapping {
    pub source_sid: String,
    /// Destination profile SID; `None` means "do not restore this user".
    pub target_sid: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleValidation {
    pub bundle_path: String,
    pub bundle_id: String,
    pub schema_version: String,
    pub schema_supported: bool,
    pub manifest_hash_ok: bool,
    pub encrypted: bool,
    pub files_checked: u64,
    pub mismatches: Vec<String>,
    pub missing: Vec<String>,
    pub errors: Vec<String>,
    pub ok: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetInfo {
    pub computer_name: String,
    pub os_name: String,
    pub os_version: String,
    pub elevated: bool,
    pub profiles: Vec<UserProfile>,
    pub running_processes: Vec<String>,
    pub free_bytes_system_drive: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreRequest {
    pub bundle_path: String,
    pub mappings: Vec<UserMapping>,
    pub selected_item_ids: Vec<String>,
    /// Per-category collision policy; categories absent use SkipExisting.
    pub policies: BTreeMap<Category, CollisionPolicy>,
    /// Categories for which the technician confirmed ReplaceAfterConfirmation.
    #[serde(default)]
    pub replace_confirmed: Vec<Category>,
    /// Categories for which system changes (drives, printers, registry,
    /// file writes) were explicitly confirmed.
    #[serde(default)]
    pub confirmed_categories: Vec<Category>,
    #[serde(default, skip_serializing)]
    pub passphrase: Option<String>,
}

/// A concrete system change shown to the technician before confirmation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SystemChange {
    RegistryValue { hive: String, key: String, value_name: String, value: String },
    MapDrive { letter: String, unc_path: String, persistent: bool },
    ConnectSharedPrinter { unc_path: String },
    AddNetworkPrinter { name: String, host_address: String, driver_name: String, port_name: String },
    ManualChecklist { text: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreAction {
    pub id: String,
    pub item_id: String,
    pub category: Category,
    pub display_name: String,
    pub source_user: Option<String>,
    pub target_user: Option<String>,
    pub target_path: Option<String>,
    pub files: u64,
    pub bytes: u64,
    pub conflicts: u64,
    pub policy: CollisionPolicy,
    pub requires_admin: bool,
    pub requires_confirmation: bool,
    pub system_changes: Vec<SystemChange>,
    pub blocked_reason: Option<String>,
    pub warnings: Vec<Warning>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestorePlan {
    pub plan_id: String,
    pub bundle_id: String,
    pub dry_run: bool,
    pub actions: Vec<RestoreAction>,
    pub total_bytes: u64,
    pub total_files: u64,
    pub total_conflicts: u64,
    pub target_free_bytes: Option<u64>,
    pub warnings: Vec<Warning>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreSummary {
    pub restore_id: String,
    pub bundle_id: String,
    pub files_written: u64,
    pub files_skipped: u64,
    pub files_renamed: u64,
    pub files_replaced: u64,
    pub failures: u64,
    pub verified: bool,
    pub outcome: String,
    pub task_results: Vec<super::capture::TaskProgress>,
    pub warnings: Vec<Warning>,
    pub report_html: String,
}
