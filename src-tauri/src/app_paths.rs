//! Portable data-path resolution.
//!
//! * If the executable's folder is writable (e.g. a USB stick), app data
//!   lives in `<exe_dir>/MigrationAssistantData/` and the default capture
//!   destination is `<exe_dir>` (bundles go to `<exe_dir>/migrations/...`).
//! * Otherwise nothing is stored by default: the technician must pick a
//!   writable destination, which is remembered for this session only unless
//!   they explicitly save a config file beside the executable.
//! * Nothing is ever written to AppData silently.

use crate::capture::preflight::is_writable_dir;
use crate::error::{AppError, AppResult, IoContext};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const DATA_DIR_NAME: &str = "MigrationAssistantData";
pub const CONFIG_FILE_NAME: &str = "migration-assistant.config.json";

#[derive(Debug, Clone, Serialize)]
pub struct PortablePaths {
    pub exe_path: String,
    pub exe_dir: String,
    pub exe_dir_writable: bool,
    /// `None` when the exe folder is read-only.
    pub data_dir: Option<String>,
    /// Suggested capture destination root, if any.
    pub default_destination: Option<String>,
    pub config_file: String,
    pub config_loaded: bool,
}

/// Optional config saved beside the executable on explicit request only.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PortableConfig {
    pub destination_root: Option<String>,
}

pub fn resolve(exe_path: &Path) -> PortablePaths {
    let exe_dir = exe_path.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
    let writable = is_writable_dir(&exe_dir);
    let config_path = exe_dir.join(CONFIG_FILE_NAME);
    let config = load_config(&config_path);
    let data_dir = writable.then(|| exe_dir.join(DATA_DIR_NAME));
    let default_destination = config
        .as_ref()
        .and_then(|c| c.destination_root.clone())
        .filter(|d| Path::new(d).is_dir())
        .or_else(|| writable.then(|| exe_dir.display().to_string()));
    PortablePaths {
        exe_path: exe_path.display().to_string(),
        exe_dir: exe_dir.display().to_string(),
        exe_dir_writable: writable,
        data_dir: data_dir.map(|d| d.display().to_string()),
        default_destination,
        config_file: config_path.display().to_string(),
        config_loaded: config.is_some(),
    }
}

pub fn load_config(path: &Path) -> Option<PortableConfig> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Save the config beside the executable (explicit user action only).
pub fn save_config(paths: &PortablePaths, config: &PortableConfig) -> AppResult<String> {
    if !paths.exe_dir_writable {
        return Err(AppError::InvalidRequest("The application folder is read-only; the configuration cannot be saved beside the executable.".into()));
    }
    let p = PathBuf::from(&paths.config_file);
    crate::util::write_json_atomic(&p, config)?;
    Ok(p.display().to_string())
}

/// Create the data dir lazily (only when something needs to be stored).
pub fn ensure_data_dir(paths: &PortablePaths) -> AppResult<Option<PathBuf>> {
    match &paths.data_dir {
        Some(d) => {
            let p = PathBuf::from(d);
            std::fs::create_dir_all(&p).at(&p)?;
            Ok(Some(p))
        }
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writable_exe_dir_is_portable_default() {
        let d = tempfile::tempdir().unwrap();
        let exe = d.path().join("MigrationAssistant.exe");
        std::fs::write(&exe, b"").unwrap();
        let p = resolve(&exe);
        assert!(p.exe_dir_writable);
        assert_eq!(p.data_dir.as_deref(), Some(d.path().join(DATA_DIR_NAME).display().to_string().as_str()));
        assert_eq!(p.default_destination.as_deref(), Some(d.path().display().to_string().as_str()));
        assert!(!p.config_loaded);
        // Nothing is created until needed.
        assert!(!d.path().join(DATA_DIR_NAME).exists());
    }

    #[test]
    fn saved_config_overrides_destination() {
        let d = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        let exe = d.path().join("ma.exe");
        let p = resolve(&exe);
        save_config(&p, &PortableConfig { destination_root: Some(dest.path().display().to_string()) }).unwrap();
        let p2 = resolve(&exe);
        assert!(p2.config_loaded);
        assert_eq!(p2.default_destination.as_deref(), Some(dest.path().display().to_string().as_str()));
    }

    #[cfg(unix)]
    #[test]
    fn read_only_exe_dir_has_no_defaults() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
        let p = resolve(&d.path().join("ma.exe"));
        let root = running_as_root();
        std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        if root {
            return; // root ignores permissions; nothing meaningful to assert
        }
        assert!(!p.exe_dir_writable);
        assert!(p.data_dir.is_none());
        assert!(p.default_destination.is_none());
        assert!(save_config(&p, &PortableConfig::default()).is_err());
    }

    #[cfg(unix)]
    fn running_as_root() -> bool {
        std::env::var("USER").map(|u| u == "root").unwrap_or(false) || std::fs::metadata("/root").map(|_| std::fs::read_dir("/root").is_ok()).unwrap_or(false)
    }
}
