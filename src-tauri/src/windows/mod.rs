//! Windows platform adapter. Uses the registry API (winreg) for profiles,
//! installed applications and mapped drives, Win32 for elevation/SID lookup/
//! drive mapping, `sysinfo` for hardware and processes, and the PowerShell
//! PrintManagement cmdlets for printers. Every adapter degrades gracefully.

pub mod powershell;
pub mod printers_parse;

#[cfg(windows)]
pub mod native;
#[cfg(windows)]
mod registry;

#[cfg(windows)]
pub use self::platform_impl::WindowsPlatform;

/// Expand `%VAR%` references using the current environment. Unknown
/// variables are left untouched.
pub fn expand_env(input: &str) -> String {
    let mut out = String::new();
    let mut rest = input;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) => {
                let var = &after[..end];
                match std::env::var(var) {
                    Ok(v) if !var.is_empty() => out.push_str(&v),
                    _ => {
                        out.push('%');
                        out.push_str(var);
                        out.push('%');
                    }
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// Windows 11 still reports "Windows 10" in ProductName; correct it from the build.
pub fn product_name_for_build(product: &str, build: u32) -> String {
    if build >= 22000 && product.contains("Windows 10") {
        product.replacen("Windows 10", "Windows 11", 1)
    } else {
        product.to_string()
    }
}

#[cfg(windows)]
mod platform_impl {
    use super::registry::*;
    use super::*;
    use crate::error::{AppError, AppResult};
    use crate::models::*;
    use crate::platform::{is_system_sid, sysinfo_adapter, Platform};
    use std::path::{Path, PathBuf};

    pub struct WindowsPlatform {
        current_profile: Option<PathBuf>,
    }

    impl WindowsPlatform {
        pub fn new() -> Self {
            Self { current_profile: std::env::var_os("USERPROFILE").map(PathBuf::from) }
        }

        fn is_current(&self, user: &UserProfile) -> bool {
            self.current_profile
                .as_ref()
                .is_some_and(|p| crate::security::safe_path::normalize_for_compare(p) == crate::security::safe_path::normalize_for_compare(&user.profile_path))
        }
    }

    impl Default for WindowsPlatform {
        fn default() -> Self {
            Self::new()
        }
    }

    impl Platform for WindowsPlatform {
        fn name(&self) -> String {
            "windows".into()
        }

        fn machine_info(&self) -> AppResult<MachineInfo> {
            let cv = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";
            let product = hklm_string(cv, "ProductName").unwrap_or_else(|| "Windows".into());
            let build_str = hklm_string(cv, "CurrentBuild").unwrap_or_default();
            let ubr = hklm_dword(cv, "UBR");
            let build_num: u32 = build_str.parse().unwrap_or(0);
            let display_version = hklm_string(cv, "DisplayVersion").or_else(|| hklm_string(cv, "ReleaseId")).unwrap_or_default();
            let edition = hklm_string(cv, "EditionID");
            let (cpu, cpus, mem) = sysinfo_adapter::cpu_and_memory();
            let (mut join_state, join_name) = native::join_information();
            if hklm_has_subkeys(r"SYSTEM\CurrentControlSet\Control\CloudDomainJoin\JoinInfo") && join_state != JoinState::Domain {
                join_state = JoinState::AzureAd;
            }
            let mut drives = sysinfo_adapter::drives();
            for (letter, _) in native::remote_drive_connections() {
                if let Some(d) = drives.iter_mut().find(|d| d.mount_point.to_uppercase().starts_with(&letter)) {
                    d.kind = DriveKind::Network;
                }
            }
            Ok(MachineInfo {
                computer_name: sysinfo_adapter::host_name(),
                os_name: product_name_for_build(&product, build_num),
                os_version: match edition {
                    Some(e) if !display_version.is_empty() => format!("{display_version} ({e})"),
                    _ => display_version,
                },
                os_build: match ubr {
                    Some(u) => format!("{build_str}.{u}"),
                    None => build_str,
                },
                architecture: std::env::var("PROCESSOR_ARCHITECTURE").unwrap_or_else(|_| std::env::consts::ARCH.into()),
                cpu_summary: cpu,
                logical_cpus: cpus,
                total_memory_bytes: mem,
                time_zone: hklm_string(r"SYSTEM\CurrentControlSet\Control\TimeZoneInformation", "TimeZoneKeyName").unwrap_or_else(|| "Unknown".into()),
                join_state,
                join_name,
                drives,
                network_adapters: sysinfo_adapter::network_adapters(),
            })
        }

        fn is_elevated(&self) -> bool {
            native::is_elevated()
        }

        fn system_roots(&self) -> Vec<PathBuf> {
            let mut v = Vec::new();
            for var in ["SystemRoot", "ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"] {
                if let Some(p) = std::env::var_os(var) {
                    let p = PathBuf::from(p);
                    if !v.contains(&p) {
                        v.push(p);
                    }
                }
            }
            if v.is_empty() {
                v.extend([r"C:\Windows", r"C:\Program Files", r"C:\Program Files (x86)"].map(PathBuf::from));
            }
            v
        }

        fn list_profiles(&self) -> AppResult<Vec<UserProfile>> {
            let list = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\ProfileList";
            let mut out = Vec::new();
            for sid in hklm_subkeys(list) {
                let key = format!(r"{list}\{sid}");
                let Some(raw_path) = hklm_string(&key, "ProfileImagePath") else { continue };
                let path = PathBuf::from(expand_env(&raw_path));
                let exists = path.is_dir();
                let folder_name = path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| sid.clone());
                let account_name = native::lookup_account(&sid).unwrap_or(folder_name);
                let (last_use, confidence) = match (hklm_dword(&key, "LocalProfileLoadTimeHigh"), hklm_dword(&key, "LocalProfileLoadTimeLow")) {
                    (Some(h), Some(l)) if h != 0 => (filetime_to_utc(((h as u64) << 32) | l as u64), Confidence::Medium),
                    _ => {
                        (std::fs::metadata(path.join("NTUSER.DAT")).and_then(|m| m.modified()).ok().map(chrono::DateTime::<chrono::Utc>::from), Confidence::Low)
                    }
                };
                let access = if !exists {
                    AccessState::NotFound
                } else {
                    match std::fs::read_dir(&path) {
                        Ok(_) => AccessState::Accessible,
                        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => AccessState::AccessDenied,
                        Err(_) => AccessState::Unknown,
                    }
                };
                let mut p = UserProfile {
                    is_system_account: is_system_sid(&sid),
                    sid,
                    account_name,
                    display_name: None,
                    profile_exists: exists,
                    is_current_user: false,
                    last_use,
                    last_use_confidence: confidence,
                    size_bytes: None,
                    access,
                    profile_path: path,
                };
                p.is_current_user = self.is_current(&p);
                out.push(p);
            }
            Ok(out)
        }

        fn known_folder(&self, user: &UserProfile, folder: KnownFolder) -> PathBuf {
            // Redirection can only be read reliably for the current user (HKCU).
            if self.is_current(user) {
                if let Some(v) = hkcu_string(r"Software\Microsoft\Windows\CurrentVersion\Explorer\User Shell Folders", folder.shell_folder_value()) {
                    let p = PathBuf::from(expand_env(&v));
                    if p.is_absolute() {
                        return p;
                    }
                }
            }
            user.profile_path.join(folder.default_dir_name())
        }

        fn roaming_app_data(&self, user: &UserProfile) -> PathBuf {
            if self.is_current(user) {
                if let Some(p) = std::env::var_os("APPDATA") {
                    return PathBuf::from(p);
                }
            }
            user.profile_path.join(r"AppData\Roaming")
        }

        fn local_app_data(&self, user: &UserProfile) -> PathBuf {
            if self.is_current(user) {
                if let Some(p) = std::env::var_os("LOCALAPPDATA") {
                    return PathBuf::from(p);
                }
            }
            user.profile_path.join(r"AppData\Local")
        }

        fn public_desktop(&self) -> Option<PathBuf> {
            std::env::var_os("PUBLIC").map(|p| PathBuf::from(p).join("Desktop"))
        }

        fn onedrive_roots(&self, user: &UserProfile) -> Vec<PathBuf> {
            let mut v = Vec::new();
            if self.is_current(user) {
                for var in ["OneDrive", "OneDriveCommercial", "OneDriveConsumer"] {
                    if let Some(p) = std::env::var_os(var) {
                        let p = PathBuf::from(p);
                        if p.is_dir() && !v.contains(&p) {
                            v.push(p);
                        }
                    }
                }
            } else if let Ok(rd) = std::fs::read_dir(&user.profile_path) {
                for e in rd.flatten() {
                    let n = e.file_name().to_string_lossy().to_string();
                    if n == "OneDrive" || n.starts_with("OneDrive - ") {
                        v.push(e.path());
                    }
                }
            }
            v
        }

        fn installed_apps(&self) -> AppResult<Vec<InstalledApp>> {
            Ok(read_uninstall_entries())
        }

        fn printers(&self) -> AppResult<Vec<PrinterInfo>> {
            let out = powershell::run_script(printers_parse::INVENTORY_SCRIPT, &[])?;
            match printers_parse::parse_inventory(&out).map_err(AppError::AdapterUnavailable)? {
                printers_parse::ParseOutcome::Unavailable => {
                    Err(AppError::AdapterUnavailable("The PrintManagement PowerShell module is not available on this PC.".into()))
                }
                printers_parse::ParseOutcome::Printers(p) => Ok(p),
            }
        }

        fn mapped_drives(&self) -> AppResult<Vec<MappedDrive>> {
            let mut drives: Vec<MappedDrive> = Vec::new();
            // Persistent mappings live under HKCU\Network\<letter>.
            for letter in hkcu_subkeys("Network") {
                let key = format!(r"Network\{letter}");
                let Some(remote) = hkcu_string(&key, "RemotePath") else { continue };
                drives.push(MappedDrive {
                    letter: format!("{}:", letter.to_uppercase()),
                    unc_path: remote,
                    provider: hkcu_string(&key, "ProviderName"),
                    persistent: true,
                    status: "Disconnected".into(),
                    label: None,
                });
            }
            for (letter, unc) in native::remote_drive_connections() {
                match drives.iter_mut().find(|d| d.letter.eq_ignore_ascii_case(&letter)) {
                    Some(d) => d.status = "Connected".into(),
                    None => drives.push(MappedDrive { letter, unc_path: unc, provider: None, persistent: false, status: "Connected".into(), label: None }),
                }
            }
            for d in &mut drives {
                let mp = format!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\MountPoints2\{}", d.unc_path.replace('\\', "#"));
                d.label = hkcu_string(&mp, "_LabelFromReg");
            }
            drives.sort_by(|a, b| a.letter.cmp(&b.letter));
            Ok(drives)
        }

        fn running_processes(&self) -> Vec<ProcessInfo> {
            sysinfo_adapter::processes()
        }

        fn disk_space(&self, path: &Path) -> Option<DiskSpace> {
            sysinfo_adapter::disk_space(path)
        }

        fn file_system_of(&self, path: &Path) -> Option<String> {
            sysinfo_adapter::file_system_of(path)
        }

        fn wallpaper_path(&self, user: &UserProfile) -> Option<PathBuf> {
            if self.is_current(user) {
                if let Some(w) = hkcu_string(r"Control Panel\Desktop", "WallPaper") {
                    let p = PathBuf::from(expand_env(&w));
                    if p.is_file() {
                        return Some(p);
                    }
                }
            }
            let transcoded = self.roaming_app_data(user).join(r"Microsoft\Windows\Themes\TranscodedWallpaper");
            transcoded.is_file().then_some(transcoded)
        }

        fn outlook_profiles(&self, user: &UserProfile) -> Vec<String> {
            if !self.is_current(user) {
                return Vec::new();
            }
            let mut v = hkcu_subkeys(r"Software\Microsoft\Office\16.0\Outlook\Profiles");
            v.extend(hkcu_subkeys(r"Software\Microsoft\Office\15.0\Outlook\Profiles"));
            v.sort();
            v.dedup();
            v
        }

        fn long_paths_enabled(&self) -> Option<bool> {
            hklm_dword(r"SYSTEM\CurrentControlSet\Control\FileSystem", "LongPathsEnabled").map(|v| v == 1)
        }

        fn printer_driver_installed(&self, driver_name: &str) -> AppResult<bool> {
            let out = powershell::run_script(printers_parse::DRIVER_EXISTS_SCRIPT, &[("MA_DRIVER", driver_name)])?;
            Ok(out.trim() == "yes")
        }

        fn map_drive(&self, letter: &str, unc_path: &str, persistent: bool) -> AppResult<()> {
            native::map_drive(letter, unc_path, persistent)
        }

        fn connect_shared_printer(&self, unc_path: &str) -> AppResult<()> {
            powershell::run_script(printers_parse::CONNECT_SHARED_SCRIPT, &[("MA_UNC", unc_path)]).map(|_| ())
        }

        fn add_network_printer(&self, name: &str, host_address: &str, driver_name: &str, port_name: &str) -> AppResult<()> {
            powershell::run_script(
                printers_parse::ADD_NETWORK_SCRIPT,
                &[("MA_NAME", name), ("MA_HOST", host_address), ("MA_DRIVER", driver_name), ("MA_PORT", port_name)],
            )
            .map(|_| ())
        }

        fn set_wallpaper(&self, path: &Path) -> AppResult<()> {
            native::set_wallpaper(path)
        }

        fn restart_elevated(&self, args: &[String]) -> AppResult<()> {
            native::restart_elevated(args)
        }

        fn open_folder(&self, path: &Path) -> AppResult<()> {
            crate::platform::fixture::open_with_system_handler(path)
        }
    }

    fn filetime_to_utc(ft: u64) -> Option<chrono::DateTime<chrono::Utc>> {
        // FILETIME: 100 ns intervals since 1601-01-01.
        const EPOCH_DIFF_SECS: i64 = 11_644_473_600;
        let secs = (ft / 10_000_000) as i64 - EPOCH_DIFF_SECS;
        let nanos = ((ft % 10_000_000) * 100) as u32;
        chrono::DateTime::from_timestamp(secs, nanos)
    }

    fn read_uninstall_entries() -> Vec<InstalledApp> {
        let sources: [(Hive, &str, &str, &str); 3] = [
            (Hive::Hklm64, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall", "x64", "Machine"),
            (Hive::Hklm32, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall", "x86", "Machine"),
            (Hive::Hkcu, r"Software\Microsoft\Windows\CurrentVersion\Uninstall", "User", "Current user"),
        ];
        let mut apps: Vec<InstalledApp> = Vec::new();
        for (hive, path, arch, scope) in sources {
            for sub in subkeys(hive, path) {
                let key = format!(r"{path}\{sub}");
                let Some(name) = string_value(hive, &key, "DisplayName").filter(|n| !n.trim().is_empty()) else { continue };
                // Skip system components and updates/patches, which are not reinstallable apps.
                if dword_value(hive, &key, "SystemComponent") == Some(1) || string_value(hive, &key, "ParentKeyName").is_some() {
                    continue;
                }
                let publisher = string_value(hive, &key, "Publisher");
                let version = string_value(hive, &key, "DisplayVersion");
                if apps.iter().any(|a| a.display_name == name && a.version == version) {
                    continue;
                }
                let arch = if arch == "User" {
                    if string_value(hive, &key, "InstallLocation").is_some_and(|l| l.contains("(x86)")) {
                        "x86"
                    } else {
                        "Unknown"
                    }
                } else {
                    arch
                };
                apps.push(crate::discovery::apps::annotate(InstalledApp {
                    display_name: name,
                    version,
                    publisher,
                    install_location: string_value(hive, &key, "InstallLocation").filter(|s| !s.is_empty()),
                    install_date: string_value(hive, &key, "InstallDate").filter(|s| !s.is_empty()),
                    uninstall_command: string_value(hive, &key, "UninstallString"),
                    architecture: arch.into(),
                    scope: scope.into(),
                    category: String::new(),
                    description: None,
                    settings_plugin: None,
                }));
            }
        }
        apps.sort_by_key(|a| a.display_name.to_lowercase());
        apps
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_known_and_keeps_unknown_vars() {
        std::env::set_var("MA_TEST_VAR", "X");
        assert_eq!(expand_env("%MA_TEST_VAR%\\a"), "X\\a");
        assert_eq!(expand_env("%NOPE_NOT_SET_123%\\a"), "%NOPE_NOT_SET_123%\\a");
        assert_eq!(expand_env("100%"), "100%");
    }

    #[test]
    fn corrects_windows_11_product_name() {
        assert_eq!(product_name_for_build("Windows 10 Pro", 22631), "Windows 11 Pro");
        assert_eq!(product_name_for_build("Windows 10 Pro", 19045), "Windows 10 Pro");
    }
}
