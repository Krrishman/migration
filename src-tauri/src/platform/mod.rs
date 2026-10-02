//! Platform abstraction. Discovery, capture and restore only talk to the OS
//! through this trait, so the whole pipeline runs against fixture trees in
//! tests and in mock mode, without administrator rights or real profiles.
//!
//! * [`crate::windows::WindowsPlatform`] – the real adapter (Windows only).
//! * [`fixture::FixturePlatform`] – reads `platform.json` and profile trees
//!   from a fixture directory; records side effects instead of applying them.

pub mod fixture;
pub mod sysinfo_adapter;

use crate::error::AppResult;
use crate::models::*;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub trait Platform: Send + Sync {
    /// "windows" or "fixture:<dir>". Shown in the UI and reports.
    fn name(&self) -> String;
    fn is_fixture(&self) -> bool {
        false
    }

    // ---------- inventory ----------
    fn machine_info(&self) -> AppResult<MachineInfo>;
    fn is_elevated(&self) -> bool;
    /// Absolute roots that are never captured (Windows, Program Files ...).
    fn system_roots(&self) -> Vec<PathBuf>;
    fn list_profiles(&self) -> AppResult<Vec<UserProfile>>;
    /// Resolved known folder for a user. Redirected locations (e.g. OneDrive
    /// known-folder move) are honoured for the current user where readable.
    fn known_folder(&self, user: &UserProfile, folder: KnownFolder) -> PathBuf {
        user.profile_path.join(folder.default_dir_name())
    }
    fn roaming_app_data(&self, user: &UserProfile) -> PathBuf {
        user.profile_path.join("AppData").join("Roaming")
    }
    fn local_app_data(&self, user: &UserProfile) -> PathBuf {
        user.profile_path.join("AppData").join("Local")
    }
    fn public_desktop(&self) -> Option<PathBuf>;
    fn onedrive_roots(&self, user: &UserProfile) -> Vec<PathBuf>;
    fn installed_apps(&self) -> AppResult<Vec<InstalledApp>>;
    fn printers(&self) -> AppResult<Vec<PrinterInfo>>;
    fn mapped_drives(&self) -> AppResult<Vec<MappedDrive>>;
    fn running_processes(&self) -> Vec<ProcessInfo>;
    fn disk_space(&self, path: &Path) -> Option<DiskSpace>;
    fn file_system_of(&self, _path: &Path) -> Option<String> {
        None
    }
    /// Current wallpaper path for a user, when readable.
    fn wallpaper_path(&self, user: &UserProfile) -> Option<PathBuf>;
    /// Outlook mail profile names (names only, never account data).
    fn outlook_profiles(&self, user: &UserProfile) -> Vec<String>;
    fn long_paths_enabled(&self) -> Option<bool>;

    // ---------- restore side effects (always explicitly confirmed) ----------
    fn printer_driver_installed(&self, driver_name: &str) -> AppResult<bool>;
    fn map_drive(&self, letter: &str, unc_path: &str, persistent: bool) -> AppResult<()>;
    fn connect_shared_printer(&self, unc_path: &str) -> AppResult<()>;
    fn add_network_printer(&self, name: &str, host_address: &str, driver_name: &str, port_name: &str) -> AppResult<()>;
    /// Sets the current user's wallpaper (HKCU\Control Panel\Desktop\WallPaper).
    fn set_wallpaper(&self, path: &Path) -> AppResult<()>;
    /// Relaunch the app elevated through the standard UAC prompt.
    fn restart_elevated(&self, args: &[String]) -> AppResult<()>;
    /// Open a folder in the platform file manager.
    fn open_folder(&self, path: &Path) -> AppResult<()>;
}

pub type SharedPlatform = Arc<dyn Platform>;

/// Select the platform adapter: an explicit fixture directory (argument
/// `--fixture <dir>` or env `MIGRATION_ASSISTANT_FIXTURE`) wins; otherwise the
/// Windows adapter on Windows and the bundled demo fixture elsewhere.
pub fn select_platform(fixture_dir: Option<PathBuf>) -> AppResult<SharedPlatform> {
    if let Some(dir) = fixture_dir {
        return Ok(Arc::new(fixture::FixturePlatform::load(&dir)?));
    }
    #[cfg(windows)]
    {
        Ok(Arc::new(crate::windows::WindowsPlatform::new()))
    }
    #[cfg(not(windows))]
    {
        Err(crate::error::AppError::AdapterUnavailable(
            "The Windows adapter is only available on Windows. Start with --fixture <dir> to use fixture data.".into(),
        ))
    }
}

/// Well-known system accounts that are never captured by default.
pub fn is_system_sid(sid: &str) -> bool {
    matches!(sid, "S-1-5-18" | "S-1-5-19" | "S-1-5-20")
        || sid.starts_with("S-1-5-80-")
        || sid.starts_with("S-1-5-82-")
        || sid.starts_with("S-1-5-90-")
        || sid.starts_with("S-1-5-96-")
}
