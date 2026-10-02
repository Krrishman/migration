//! Discovery-side domain models: what the scanner found on the source PC.

use super::common::*;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Where an item comes from: a filesystem path or a configuration reference
/// (registry key name, printer queue, etc.). Shown verbatim in the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SourceRef {
    Path { path: PathBuf },
    Config { reference: String },
}

/// A Windows known folder. Used to map files to the equivalent folder for the
/// destination user (which may be redirected, e.g. to OneDrive).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnownFolder {
    Desktop,
    Documents,
    Downloads,
    Pictures,
    Videos,
    Music,
    Favorites,
    Links,
    Contacts,
    SavedGames,
}

impl KnownFolder {
    pub const ALL: [KnownFolder; 10] = [
        KnownFolder::Desktop,
        KnownFolder::Documents,
        KnownFolder::Downloads,
        KnownFolder::Pictures,
        KnownFolder::Videos,
        KnownFolder::Music,
        KnownFolder::Favorites,
        KnownFolder::Links,
        KnownFolder::Contacts,
        KnownFolder::SavedGames,
    ];

    /// Default folder name relative to the profile root.
    pub fn default_dir_name(self) -> &'static str {
        match self {
            KnownFolder::Desktop => "Desktop",
            KnownFolder::Documents => "Documents",
            KnownFolder::Downloads => "Downloads",
            KnownFolder::Pictures => "Pictures",
            KnownFolder::Videos => "Videos",
            KnownFolder::Music => "Music",
            KnownFolder::Favorites => "Favorites",
            KnownFolder::Links => "Links",
            KnownFolder::Contacts => "Contacts",
            KnownFolder::SavedGames => "Saved Games",
        }
    }

    /// Value name under HKCU\...\Explorer\User Shell Folders.
    pub fn shell_folder_value(self) -> &'static str {
        match self {
            KnownFolder::Desktop => "Desktop",
            KnownFolder::Documents => "Personal",
            KnownFolder::Downloads => "{374DE290-123F-4565-9164-39C4925E467B}",
            KnownFolder::Pictures => "My Pictures",
            KnownFolder::Videos => "My Video",
            KnownFolder::Music => "My Music",
            KnownFolder::Favorites => "Favorites",
            KnownFolder::Links => "{BFB9D5E0-C6A9-404C-B2B2-AE6DB6AF4968}",
            KnownFolder::Contacts => "{56784854-C6CB-462B-8169-88E350ACB882}",
            KnownFolder::SavedGames => "{4C5C32FF-BB9D-43B0-B5B4-2D72E54EAAA4}",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserKind {
    Chrome,
    Edge,
    Firefox,
    /// Generic Chromium-family browser with a technician-confirmed profile root.
    Chromium,
}

impl BrowserKind {
    pub fn label(self) -> &'static str {
        match self {
            BrowserKind::Chrome => "Google Chrome",
            BrowserKind::Edge => "Microsoft Edge",
            BrowserKind::Firefox => "Mozilla Firefox",
            BrowserKind::Chromium => "Chromium-family browser",
        }
    }
}

/// What a selected item restores as. Persisted in the manifest so a restore
/// never has to re-derive intent from paths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RestoreKind {
    KnownFolder {
        folder: KnownFolder,
    },
    CustomFolder,
    /// Locally present OneDrive files; restored to "Migrated Files" to avoid sync conflicts.
    OneDriveLocal,
    PublicDesktop,
    StartMenuShortcuts,
    TaskbarPins,
    QuickAccess,
    RecentItems,
    BrowserProfile {
        browser: BrowserKind,
        profile_dir: String,
    },
    OutlookSignatures,
    OutlookTemplates,
    OutlookStationery,
    PstFile,
    OfficeTemplates,
    Wallpaper,
    Themes,
    StickyNotes,
    MappedDrives,
    Printers,
    /// JSON inventory only; never restored automatically.
    Inventory {
        name: String,
    },
}

/// Information the capture engine needs to collect an item. This stays on the
/// backend: the UI selects items by id only, so it can never inject paths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ItemPayload {
    /// Recursive folder copy subject to exclusion rules.
    Folder { root: PathBuf },
    /// Allow-listed files and directories under `root` (browsers, Sticky Notes).
    AllowList { root: PathBuf, files: Vec<String>, dirs: Vec<String> },
    /// Files matching extensions directly inside `root` (non-recursive).
    FilesByExtension { root: PathBuf, extensions: Vec<String> },
    /// A fixed list of absolute files (wallpaper, PST files).
    Files { files: Vec<PathBuf> },
    /// Structured inventory written as JSON into the bundle.
    Inventory { name: String },
    /// Display-only entry, never captured.
    None,
}

/// A single selectable row in the dashboard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryItem {
    pub id: String,
    pub category: Category,
    pub display_name: String,
    pub description: String,
    pub source: SourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<UserRef>,
    /// Bytes, when estimable. `None` means "unknown", never zero.
    pub estimated_size: Option<u64>,
    pub item_count: Option<u64>,
    pub access: AccessState,
    pub support: SupportLevel,
    pub selected_by_default: bool,
    /// Privacy-sensitive (e.g. recent items). Never preselected and only
    /// selectable after the technician enables privacy-sensitive items.
    pub sensitive: bool,
    /// Must be explicitly ticked; never included by "Select all safe items"
    /// (app plug-ins, PST files, Sticky Notes, OneDrive roots).
    pub opt_in_only: bool,
    pub requires_admin: bool,
    pub warnings: Vec<Warning>,
    pub restore_notes: Vec<String>,
    /// Exactly what is included, shown in the details pane.
    pub includes: Vec<String>,
    /// Exactly what is excluded and why, shown in the details pane.
    pub excludes: Vec<String>,
    pub restore_kind: RestoreKind,
    pub payload: ItemPayload,
}

impl DiscoveryItem {
    pub fn source_display(&self) -> String {
        match &self.source {
            SourceRef::Path { path } => path.display().to_string(),
            SourceRef::Config { reference } => reference.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriveInfo {
    pub mount_point: String,
    pub label: String,
    pub file_system: String,
    pub kind: DriveKind,
    pub total_bytes: u64,
    pub free_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DriveKind {
    Fixed,
    Removable,
    Network,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkAdapterInfo {
    pub name: String,
    /// MAC addresses are inventory, not secrets; IPs are intentionally omitted.
    pub mac_address: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JoinState {
    Workgroup,
    Domain,
    AzureAd,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MachineInfo {
    pub computer_name: String,
    pub os_name: String,
    pub os_version: String,
    pub os_build: String,
    pub architecture: String,
    pub cpu_summary: String,
    pub logical_cpus: usize,
    pub total_memory_bytes: u64,
    pub time_zone: String,
    pub join_state: JoinState,
    pub join_name: Option<String>,
    pub drives: Vec<DriveInfo>,
    pub network_adapters: Vec<NetworkAdapterInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserProfile {
    pub sid: String,
    pub account_name: String,
    pub display_name: Option<String>,
    pub profile_path: PathBuf,
    pub profile_exists: bool,
    pub is_current_user: bool,
    pub is_system_account: bool,
    pub last_use: Option<chrono::DateTime<chrono::Utc>>,
    pub last_use_confidence: Confidence,
    pub size_bytes: Option<u64>,
    pub access: AccessState,
}

impl UserProfile {
    pub fn user_ref(&self) -> UserRef {
        UserRef { sid: self.sid.clone(), account_name: self.account_name.clone() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrinterConnection {
    Usb,
    Local,
    Network,
    Shared,
    Wsd,
    Virtual,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrinterInfo {
    pub name: String,
    pub share_name: Option<String>,
    pub port_name: String,
    pub port_type: Option<String>,
    pub host_address: Option<String>,
    pub unc_path: Option<String>,
    pub connection: PrinterConnection,
    pub driver_name: String,
    pub driver_version: Option<String>,
    pub is_default: bool,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MappedDrive {
    pub letter: String,
    pub unc_path: String,
    pub provider: Option<String>,
    pub persistent: bool,
    pub status: String,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledApp {
    pub display_name: String,
    pub version: Option<String>,
    pub publisher: Option<String>,
    pub install_location: Option<String>,
    pub install_date: Option<String>,
    /// Technician-only. Never shown in redacted reports.
    pub uninstall_command: Option<String>,
    pub architecture: String,
    pub scope: String,
    pub category: String,
    pub description: Option<String>,
    pub settings_plugin: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserProfileInfo {
    pub browser: BrowserKind,
    pub owner: UserRef,
    pub profile_dir: String,
    pub profile_name: Option<String>,
    pub path: PathBuf,
    pub size_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessInfo {
    pub name: String,
    pub pid: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskSpace {
    pub path: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanResult {
    pub scan_id: String,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub finished_at: chrono::DateTime<chrono::Utc>,
    pub machine: MachineInfo,
    pub elevated: bool,
    pub users: Vec<UserProfile>,
    pub items: Vec<DiscoveryItem>,
    pub printers: Vec<PrinterInfo>,
    pub mapped_drives: Vec<MappedDrive>,
    pub applications: Vec<InstalledApp>,
    pub browser_profiles: Vec<BrowserProfileInfo>,
    pub running_processes: Vec<ProcessInfo>,
    pub warnings: Vec<Warning>,
    /// Data source label: "windows" or "fixture:<path>". Always shown in UI.
    pub platform: String,
}

/// Stage events emitted while scanning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanProgress {
    pub stage: String,
    pub stage_index: usize,
    pub stage_count: usize,
    pub message: String,
}
