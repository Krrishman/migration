//! Fixture platform adapter. Reads a `platform.json` description plus real
//! profile directory trees from a fixture folder. Used by tests, by
//! `--fixture` mock mode and for UI development without admin rights.
//!
//! Side effects (drive mapping, printers, wallpaper, elevation) are recorded
//! in an in-memory journal and never touch the host system.

use super::{sysinfo_adapter, Platform};
use crate::error::{AppError, AppResult, IoContext};
use crate::models::*;
use parking_lot::Mutex;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
pub struct FixtureProfile {
    pub sid: String,
    pub account_name: String,
    #[serde(default)]
    pub display_name: Option<String>,
    /// Relative to the fixture root.
    pub profile_dir: String,
    #[serde(default)]
    pub last_use: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub is_current_user: bool,
    #[serde(default)]
    pub access_denied: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FixtureMachine {
    pub computer_name: String,
    pub os_name: String,
    pub os_version: String,
    pub os_build: String,
    #[serde(default = "default_arch")]
    pub architecture: String,
    #[serde(default)]
    pub cpu_summary: Option<String>,
    #[serde(default)]
    pub total_memory_bytes: Option<u64>,
    #[serde(default = "default_tz")]
    pub time_zone: String,
    #[serde(default = "default_join")]
    pub join_state: JoinState,
    #[serde(default)]
    pub join_name: Option<String>,
}

fn default_arch() -> String {
    "x64".into()
}
fn default_tz() -> String {
    "Pacific Standard Time".into()
}
fn default_join() -> JoinState {
    JoinState::Workgroup
}

#[derive(Debug, Clone, Deserialize)]
pub struct FixtureSpec {
    pub machine: FixtureMachine,
    #[serde(default)]
    pub elevated: bool,
    pub profiles: Vec<FixtureProfile>,
    #[serde(default)]
    pub public_desktop: Option<String>,
    #[serde(default)]
    pub system_roots: Vec<String>,
    #[serde(default)]
    pub installed_apps: Vec<InstalledApp>,
    #[serde(default)]
    pub printers: Vec<PrinterInfo>,
    #[serde(default)]
    pub printer_drivers: Vec<String>,
    #[serde(default)]
    pub mapped_drives: Vec<MappedDrive>,
    #[serde(default)]
    pub processes: Vec<String>,
    /// SID -> fixture-relative wallpaper path.
    #[serde(default)]
    pub wallpapers: HashMap<String, String>,
    /// SID -> Outlook profile names.
    #[serde(default)]
    pub outlook_profiles: HashMap<String, Vec<String>>,
    /// SID -> fixture-relative OneDrive roots.
    #[serde(default)]
    pub onedrive_roots: HashMap<String, Vec<String>>,
    #[serde(default)]
    pub free_space_override: Option<u64>,
    #[serde(default)]
    pub long_paths_enabled: Option<bool>,
    #[serde(default)]
    pub printers_unavailable: bool,
    /// Fixture-relative files that behave as if locked by another process.
    #[serde(default)]
    pub locked_files: Vec<String>,
}

pub struct FixturePlatform {
    root: PathBuf,
    spec: FixtureSpec,
    processes: Mutex<Vec<String>>,
    journal: Mutex<Vec<String>>,
    free_space_override: Mutex<Option<u64>>,
    locked: Mutex<Vec<String>>,
}

impl FixturePlatform {
    pub fn load(root: &Path) -> AppResult<Self> {
        let spec_path = root.join("platform.json");
        let raw = std::fs::read(&spec_path).at(&spec_path)?;
        let spec: FixtureSpec = serde_json::from_slice(&raw)
            .map_err(|e| AppError::InvalidRequest(format!("invalid fixture {}: {e}", spec_path.display())))?;
        let root = std::fs::canonicalize(root).at(root)?;
        Ok(Self {
            processes: Mutex::new(spec.processes.clone()),
            free_space_override: Mutex::new(spec.free_space_override),
            locked: Mutex::new(spec.locked_files.clone()),
            root,
            spec,
            journal: Mutex::new(Vec::new()),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn resolve(&self, rel: &str) -> PathBuf {
        self.root.join(rel.replace('\\', "/"))
    }

    /// Replace the simulated running-process list (tests).
    pub fn set_processes(&self, names: &[&str]) {
        *self.processes.lock() = names.iter().map(|s| s.to_string()).collect();
    }

    /// Simulate low disk space (tests).
    pub fn set_free_space(&self, bytes: Option<u64>) {
        *self.free_space_override.lock() = bytes;
    }

    /// Simulate files locked by another process (fixture-relative paths).
    pub fn set_locked_files(&self, rels: &[&str]) {
        *self.locked.lock() = rels.iter().map(|s| s.to_string()).collect();
    }

    /// Recorded side effects, e.g. `map_drive Z: \\srv\share persistent=true`.
    pub fn journal(&self) -> Vec<String> {
        self.journal.lock().clone()
    }
}

impl Platform for FixturePlatform {
    fn name(&self) -> String {
        format!("fixture:{}", self.root.display())
    }

    fn is_fixture(&self) -> bool {
        true
    }

    fn machine_info(&self) -> AppResult<MachineInfo> {
        let m = &self.spec.machine;
        let (cpu, cpus, mem) = sysinfo_adapter::cpu_and_memory();
        Ok(MachineInfo {
            computer_name: m.computer_name.clone(),
            os_name: m.os_name.clone(),
            os_version: m.os_version.clone(),
            os_build: m.os_build.clone(),
            architecture: m.architecture.clone(),
            cpu_summary: m.cpu_summary.clone().unwrap_or(cpu),
            logical_cpus: cpus,
            total_memory_bytes: m.total_memory_bytes.unwrap_or(mem),
            time_zone: m.time_zone.clone(),
            join_state: m.join_state,
            join_name: m.join_name.clone(),
            drives: vec![DriveInfo {
                mount_point: "C:\\".into(),
                label: "Fixture".into(),
                file_system: "NTFS".into(),
                kind: DriveKind::Fixed,
                total_bytes: 512 * 1024 * 1024 * 1024,
                free_bytes: self.disk_space(&self.root).map(|d| d.free_bytes).unwrap_or(0),
            }],
            network_adapters: vec![NetworkAdapterInfo { name: "Ethernet (fixture)".into(), mac_address: "00:15:5D:00:00:01".into() }],
        })
    }

    fn is_elevated(&self) -> bool {
        self.spec.elevated
    }

    fn system_roots(&self) -> Vec<PathBuf> {
        self.spec.system_roots.iter().map(|r| self.resolve(r)).collect()
    }

    fn list_profiles(&self) -> AppResult<Vec<UserProfile>> {
        Ok(self
            .spec
            .profiles
            .iter()
            .map(|p| {
                let path = self.resolve(&p.profile_dir);
                let exists = path.is_dir();
                UserProfile {
                    sid: p.sid.clone(),
                    account_name: p.account_name.clone(),
                    display_name: p.display_name.clone(),
                    profile_exists: exists,
                    is_current_user: p.is_current_user,
                    is_system_account: super::is_system_sid(&p.sid),
                    last_use: p.last_use,
                    last_use_confidence: if p.last_use.is_some() { Confidence::Medium } else { Confidence::Low },
                    size_bytes: None,
                    access: if !exists {
                        AccessState::NotFound
                    } else if p.access_denied {
                        AccessState::AccessDenied
                    } else {
                        AccessState::Accessible
                    },
                    profile_path: path,
                }
            })
            .collect())
    }

    fn public_desktop(&self) -> Option<PathBuf> {
        self.spec.public_desktop.as_ref().map(|p| self.resolve(p))
    }

    fn onedrive_roots(&self, user: &UserProfile) -> Vec<PathBuf> {
        self.spec.onedrive_roots.get(&user.sid).map(|v| v.iter().map(|p| self.resolve(p)).collect()).unwrap_or_default()
    }

    fn installed_apps(&self) -> AppResult<Vec<InstalledApp>> {
        Ok(self.spec.installed_apps.clone())
    }

    fn printers(&self) -> AppResult<Vec<PrinterInfo>> {
        if self.spec.printers_unavailable {
            return Err(AppError::AdapterUnavailable("PrintManagement cmdlets are not available (fixture)".into()));
        }
        Ok(self.spec.printers.clone())
    }

    fn mapped_drives(&self) -> AppResult<Vec<MappedDrive>> {
        Ok(self.spec.mapped_drives.clone())
    }

    fn running_processes(&self) -> Vec<ProcessInfo> {
        self.processes.lock().iter().enumerate().map(|(i, n)| ProcessInfo { name: n.clone(), pid: 1000 + i as u32 }).collect()
    }

    fn disk_space(&self, path: &Path) -> Option<DiskSpace> {
        let real = sysinfo_adapter::disk_space(path);
        match *self.free_space_override.lock() {
            Some(free) => Some(DiskSpace {
                path: path.display().to_string(),
                total_bytes: real.as_ref().map(|r| r.total_bytes).unwrap_or(free).max(free),
                free_bytes: free,
            }),
            None => real,
        }
    }

    fn file_system_of(&self, _path: &Path) -> Option<String> {
        Some("NTFS".into())
    }

    fn wallpaper_path(&self, user: &UserProfile) -> Option<PathBuf> {
        self.spec.wallpapers.get(&user.sid).map(|p| self.resolve(p))
    }

    fn outlook_profiles(&self, user: &UserProfile) -> Vec<String> {
        self.spec.outlook_profiles.get(&user.sid).cloned().unwrap_or_default()
    }

    fn long_paths_enabled(&self) -> Option<bool> {
        self.spec.long_paths_enabled
    }

    fn injected_open_error(&self, path: &Path) -> Option<std::io::Error> {
        let locked = self.locked.lock();
        locked
            .iter()
            .any(|l| crate::security::safe_path::normalize_for_compare(&self.resolve(l)) == crate::security::safe_path::normalize_for_compare(path))
            .then(|| std::io::Error::new(std::io::ErrorKind::ResourceBusy, "The process cannot access the file because it is being used by another process (simulated)"))
    }

    fn printer_driver_installed(&self, driver_name: &str) -> AppResult<bool> {
        Ok(self.spec.printer_drivers.iter().any(|d| d.eq_ignore_ascii_case(driver_name)))
    }

    fn map_drive(&self, letter: &str, unc_path: &str, persistent: bool) -> AppResult<()> {
        self.journal.lock().push(format!("map_drive {letter} {unc_path} persistent={persistent}"));
        Ok(())
    }

    fn connect_shared_printer(&self, unc_path: &str) -> AppResult<()> {
        self.journal.lock().push(format!("connect_shared_printer {unc_path}"));
        Ok(())
    }

    fn add_network_printer(&self, name: &str, host_address: &str, driver_name: &str, port_name: &str) -> AppResult<()> {
        if !self.printer_driver_installed(driver_name)? {
            return Err(AppError::InvalidRequest(format!("driver '{driver_name}' is not installed")));
        }
        self.journal.lock().push(format!("add_network_printer {name} {host_address} {driver_name} {port_name}"));
        Ok(())
    }

    fn set_wallpaper(&self, path: &Path) -> AppResult<()> {
        self.journal.lock().push(format!("set_wallpaper {}", path.display()));
        Ok(())
    }

    fn restart_elevated(&self, args: &[String]) -> AppResult<()> {
        self.journal.lock().push(format!("restart_elevated {}", args.join(" ")));
        Err(AppError::AdapterUnavailable("Elevation is simulated in fixture mode; nothing was restarted.".into()))
    }

    fn open_folder(&self, path: &Path) -> AppResult<()> {
        self.journal.lock().push(format!("open_folder {}", path.display()));
        open_with_system_handler(path)
    }
}

/// Open a folder with the OS file manager (explorer.exe / xdg-open / open).
pub fn open_with_system_handler(path: &Path) -> AppResult<()> {
    if !path.is_dir() {
        return Err(AppError::NotFound(path.display().to_string()));
    }
    #[cfg(windows)]
    let program = "explorer.exe";
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(all(unix, not(target_os = "macos")))]
    let program = "xdg-open";
    std::process::Command::new(program).arg(path).spawn().map(|_| ()).at(path)
}

/// Open a local file (report) with its default application.
pub fn open_file_with_system_handler(path: &Path) -> AppResult<()> {
    if !path.is_file() {
        return Err(AppError::NotFound(path.display().to_string()));
    }
    #[cfg(windows)]
    let program = "explorer.exe";
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(all(unix, not(target_os = "macos")))]
    let program = "xdg-open";
    std::process::Command::new(program).arg(path).spawn().map(|_| ()).at(path)
}
