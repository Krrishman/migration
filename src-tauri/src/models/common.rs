//! Shared value types used across discovery, capture, restore and reporting.

use serde::{Deserialize, Serialize};

/// Top-level dashboard category. "Overview" and "Warnings" are UI views,
/// not categories, so they are intentionally absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    UsersFiles,
    Browsers,
    OutlookEmail,
    Personalization,
    Printers,
    NetworkDrives,
    DesktopShortcuts,
    ApplicationSettings,
    InstalledApplications,
    SystemInventory,
}

impl Category {
    pub const ALL: [Category; 10] = [
        Category::UsersFiles,
        Category::Browsers,
        Category::OutlookEmail,
        Category::Personalization,
        Category::Printers,
        Category::NetworkDrives,
        Category::DesktopShortcuts,
        Category::ApplicationSettings,
        Category::InstalledApplications,
        Category::SystemInventory,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Category::UsersFiles => "Users & Files",
            Category::Browsers => "Browsers",
            Category::OutlookEmail => "Outlook & Email",
            Category::Personalization => "Personalization",
            Category::Printers => "Printers",
            Category::NetworkDrives => "Network Drives",
            Category::DesktopShortcuts => "Desktop & Shortcuts",
            Category::ApplicationSettings => "Application Settings",
            Category::InstalledApplications => "Installed Applications",
            Category::SystemInventory => "System Inventory",
        }
    }

    /// Restore ordering. Lower runs first. Mirrors the documented dependency
    /// order: files, desktop, browsers, Outlook, personalization, drives,
    /// printers, then optional plug-ins.
    pub fn restore_order(self) -> u8 {
        match self {
            Category::UsersFiles => 10,
            Category::DesktopShortcuts => 20,
            Category::Browsers => 30,
            Category::OutlookEmail => 40,
            Category::Personalization => 50,
            Category::NetworkDrives => 60,
            Category::Printers => 70,
            Category::ApplicationSettings => 80,
            Category::InstalledApplications => 90,
            Category::SystemInventory => 95,
        }
    }
}

/// How completely Migration Assistant can carry an item from source to target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupportLevel {
    /// Captured and restored as files with verification.
    Supported,
    /// Some parts are captured/restored; the item details explain what is not.
    Partial,
    /// Recorded in reports only; nothing is restored automatically.
    InventoryOnly,
    /// Shown for transparency; cannot be selected for capture.
    Unsupported,
}

impl SupportLevel {
    pub fn is_capturable(self) -> bool {
        !matches!(self, SupportLevel::Unsupported)
    }
}

/// Result of probing whether the current process can read an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessState {
    Accessible,
    /// Some children were unreadable; the readable remainder can be captured.
    PartiallyAccessible,
    /// Access denied for the current token. Elevation may help; ACLs are never bypassed.
    AccessDenied,
    /// The item exists but its files are held open by a running process.
    Locked,
    NotFound,
    /// The item could not be probed (e.g. an adapter is unavailable).
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

/// A user-facing, machine-identifiable warning. `code` is stable and used by
/// tests and the UI for grouping; `message` is plain language.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Warning {
    pub code: WarningCode,
    pub severity: Severity,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

impl Warning {
    pub fn new(code: WarningCode, severity: Severity, message: impl Into<String>) -> Self {
        Self { code, severity, message: message.into(), path: None }
    }
    pub fn warn(code: WarningCode, message: impl Into<String>) -> Self {
        Self::new(code, Severity::Warning, message)
    }
    pub fn info(code: WarningCode, message: impl Into<String>) -> Self {
        Self::new(code, Severity::Info, message)
    }
    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WarningCode {
    AccessDenied,
    LockedFile,
    CloudPlaceholder,
    EfsEncrypted,
    LongPath,
    ReparsePointSkipped,
    ExcludedPath,
    SensitiveExcluded,
    BrowserRunning,
    ApplicationRunning,
    AdminRequired,
    AdapterUnavailable,
    LowDiskSpace,
    PrivacySensitive,
    LargeItem,
    VersionDependent,
    ServerSyncedCache,
    DriverRequired,
    CredentialsNotMigrated,
    HashMismatch,
    Retried,
    Skipped,
    Conflict,
    Other,
}

/// Owner of a per-user item.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UserRef {
    pub sid: String,
    pub account_name: String,
}

/// How confident we are in a discovered value (used for "last logon" etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    High,
    Medium,
    Low,
}
