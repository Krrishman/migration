//! Browser discovery through an extensible [`BrowserProvider`] trait.
//!
//! Capture is **allow-list based**: only named, non-secret files are copied
//! (bookmarks, history, preferences, extension code). Passwords, cookies,
//! tokens, session state, autofill/payment data and the browser's encryption
//! key container are never read or copied — see `security::exclusions`.

use super::*;
use crate::error::AppResult;
use std::sync::Arc;

pub trait BrowserProvider: Send + Sync {
    fn kind(&self) -> BrowserKind;
    fn display_name(&self) -> String;
    /// Executable names used for the closed-browser preflight.
    fn process_names(&self) -> &[&'static str];
    /// Profile-container roots for a user (e.g. `...\Chrome\User Data`).
    fn profile_roots(&self, platform: &dyn Platform, user: &UserProfile) -> Vec<PathBuf>;
    /// (folder name, display name if safely readable, absolute path)
    fn enumerate_profiles(&self, root: &Path) -> Vec<(String, Option<String>, PathBuf)>;
    fn allow_list(&self, include_cache: bool) -> AllowList;
    fn supported_components(&self) -> Vec<&'static str>;
    fn excluded_components(&self) -> Vec<&'static str>;
    fn restore_notes(&self) -> Vec<String>;

    /// Returns a warning if the browser is running (files may be locked or inconsistent).
    fn preflight(&self, running: &[String]) -> Option<Warning> {
        if running.is_empty() {
            return None;
        }
        Some(Warning::warn(
            WarningCode::BrowserRunning,
            format!(
                "{} is running ({}). Close it before capture so its databases are consistent. Migration Assistant never closes it without your separate confirmation.",
                self.display_name(),
                running.join(", ")
            ),
        ))
    }
}

/// Chromium-family provider (Chrome, Edge, or a technician-confirmed root).
pub struct ChromiumProvider {
    kind: BrowserKind,
    name: String,
    /// Path of "User Data" relative to %LOCALAPPDATA%, or an absolute root for the generic provider.
    rel_root: Option<&'static str>,
    custom_root: Option<PathBuf>,
    processes: &'static [&'static str],
}

impl ChromiumProvider {
    pub fn chrome() -> Self {
        Self { kind: BrowserKind::Chrome, name: "Google Chrome".into(), rel_root: Some("Google/Chrome/User Data"), custom_root: None, processes: &["chrome.exe"] }
    }
    pub fn edge() -> Self {
        Self { kind: BrowserKind::Edge, name: "Microsoft Edge".into(), rel_root: Some("Microsoft/Edge/User Data"), custom_root: None, processes: &["msedge.exe"] }
    }
    /// Generic Chromium-family browser whose "User Data" root the technician
    /// selected and confirmed manually (e.g. Brave, Vivaldi).
    pub fn generic(name: String, root: PathBuf) -> Self {
        Self { kind: BrowserKind::Chromium, name, rel_root: None, custom_root: Some(root), processes: &[] }
    }

    /// A folder is a plausible Chromium "User Data" root if it contains a
    /// profile folder with a Preferences file.
    pub fn looks_like_user_data(root: &Path) -> bool {
        ["Default", "Profile 1"].iter().any(|p| root.join(p).join("Preferences").is_file())
    }
}

/// Read only `profile.name` from a Chromium Preferences file.
fn chromium_profile_name(profile_dir: &Path) -> Option<String> {
    let raw = std::fs::read(profile_dir.join("Preferences")).ok()?;
    // Preferences can be several MB; parse leniently and only extract one field.
    let v: serde_json::Value = serde_json::from_slice(&raw).ok()?;
    v.get("profile")?.get("name")?.as_str().map(str::to_string).filter(|s| !s.is_empty())
}

impl BrowserProvider for ChromiumProvider {
    fn kind(&self) -> BrowserKind {
        self.kind
    }
    fn display_name(&self) -> String {
        self.name.clone()
    }
    fn process_names(&self) -> &[&'static str] {
        self.processes
    }
    fn profile_roots(&self, platform: &dyn Platform, user: &UserProfile) -> Vec<PathBuf> {
        if let Some(r) = &self.custom_root {
            return if crate::security::safe_path::is_within(&user.profile_path, r) { vec![r.clone()] } else { vec![] };
        }
        let mut p = platform.local_app_data(user);
        for part in self.rel_root.unwrap_or_default().split('/') {
            p.push(part);
        }
        if p.is_dir() { vec![p] } else { vec![] }
    }
    fn enumerate_profiles(&self, root: &Path) -> Vec<(String, Option<String>, PathBuf)> {
        let mut v = Vec::new();
        if let Ok(rd) = std::fs::read_dir(root) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                let path = e.path();
                let is_profile = (name == "Default" || name.starts_with("Profile ")) && path.join("Preferences").is_file();
                if is_profile {
                    v.push((name, chromium_profile_name(&path), path));
                }
            }
        }
        v.sort_by(|a, b| a.0.cmp(&b.0));
        v
    }
    fn allow_list(&self, include_cache: bool) -> AllowList {
        let mut dirs = vec!["Extensions".to_string()];
        if include_cache {
            dirs.push("Cache".into());
        }
        AllowList {
            files: ["Bookmarks", "Bookmarks.bak", "History", "History-journal", "Favicons", "Favicons-journal", "Top Sites", "Top Sites-journal", "Preferences", "Custom Dictionary.txt", "Shortcuts", "Visited Links"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            dirs,
        }
    }
    fn supported_components(&self) -> Vec<&'static str> {
        vec!["Bookmarks/favorites", "Browsing history", "Top sites and favicons", "Custom dictionary", "Preferences (partial)", "Installed extension files (partial)"]
    }
    fn excluded_components(&self) -> Vec<&'static str> {
        vec![
            "Saved passwords (Login Data)",
            "Cookies and site sign-ins",
            "Autofill and payment data (Web Data)",
            "Sync/account tokens",
            "Session state and site storage (Local Storage, IndexedDB)",
            "Browser encryption key container (Local State)",
            "Machine-bound protected preferences (Secure Preferences)",
            "Caches (unless explicitly enabled)",
        ]
    }
    fn restore_notes(&self) -> Vec<String> {
        vec![
            "Bookmarks are also exported to an importable bookmarks HTML file on the destination Desktop.".into(),
            "Saved passwords, cookies and sign-ins are protected by Windows/browser encryption and cannot be migrated. Use the browser's own account sync or password export.".into(),
            "Extensions may need to be re-enabled or reinstalled from the store.".into(),
        ]
    }
}

pub struct FirefoxProvider;

/// Parse profiles.ini into (folder, name, absolute path).
pub fn parse_profiles_ini(ini: &str, firefox_root: &Path) -> Vec<(String, Option<String>, PathBuf)> {
    let mut out = Vec::new();
    let mut section = String::new();
    let mut name: Option<String> = None;
    let mut path: Option<String> = None;
    let mut relative = true;
    let mut flush = |section: &str, name: &mut Option<String>, path: &mut Option<String>, relative: &mut bool| {
        if section.starts_with("Profile") {
            if let Some(p) = path.take() {
                let abs = if *relative { firefox_root.join(p.replace('\\', "/")) } else { PathBuf::from(&p) };
                let folder = abs.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or(p);
                out.push((folder, name.take(), abs));
            }
        }
        *name = None;
        *path = None;
        *relative = true;
    };
    for line in ini.lines().map(str::trim) {
        if line.starts_with('[') && line.ends_with(']') {
            flush(&section, &mut name, &mut path, &mut relative);
            section = line[1..line.len() - 1].to_string();
        } else if let Some((k, v)) = line.split_once('=') {
            match k.trim() {
                "Name" => name = Some(v.trim().to_string()),
                "Path" => path = Some(v.trim().to_string()),
                "IsRelative" => relative = v.trim() == "1",
                _ => {}
            }
        }
    }
    flush(&section, &mut name, &mut path, &mut relative);
    out
}

impl BrowserProvider for FirefoxProvider {
    fn kind(&self) -> BrowserKind {
        BrowserKind::Firefox
    }
    fn display_name(&self) -> String {
        "Mozilla Firefox".into()
    }
    fn process_names(&self) -> &[&'static str] {
        &["firefox.exe"]
    }
    fn profile_roots(&self, platform: &dyn Platform, user: &UserProfile) -> Vec<PathBuf> {
        let p = platform.roaming_app_data(user).join("Mozilla").join("Firefox");
        if p.join("profiles.ini").is_file() { vec![p] } else { vec![] }
    }
    fn enumerate_profiles(&self, root: &Path) -> Vec<(String, Option<String>, PathBuf)> {
        let Ok(ini) = std::fs::read_to_string(root.join("profiles.ini")) else { return vec![] };
        parse_profiles_ini(&ini, root)
            .into_iter()
            // Only profiles inside the Firefox root are offered (no arbitrary absolute paths).
            .filter(|(_, _, p)| p.is_dir() && crate::security::safe_path::is_within(root, p))
            .collect()
    }
    fn allow_list(&self, include_cache: bool) -> AllowList {
        let mut dirs = vec!["bookmarkbackups".to_string(), "extensions".into(), "chrome".into()];
        if include_cache {
            dirs.push("cache2".into());
        }
        AllowList {
            files: [
                "places.sqlite", "places.sqlite-wal", "favicons.sqlite", "favicons.sqlite-wal", "prefs.js", "user.js", "extensions.json", "addons.json",
                "search.json.mozlz4", "handlers.json", "xulstore.json", "containers.json", "permissions.sqlite", "content-prefs.sqlite", "persdict.dat",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            dirs,
        }
    }
    fn supported_components(&self) -> Vec<&'static str> {
        vec!["Bookmarks and history (places.sqlite)", "Bookmark backups", "Preferences (prefs.js)", "Search engines", "Containers", "Site permissions", "Extensions (partial)"]
    }
    fn excluded_components(&self) -> Vec<&'static str> {
        vec![
            "Saved passwords (logins.json, key4.db)",
            "Cookies (cookies.sqlite)",
            "Certificates and security devices (cert9.db, pkcs11.txt)",
            "Form/autofill history",
            "Firefox Account sign-in (signedInUser.json)",
            "Session state and site storage",
            "Caches (unless explicitly enabled)",
        ]
    }
    fn restore_notes(&self) -> Vec<String> {
        vec![
            "Bookmarks are exported to an importable bookmarks HTML file on the destination Desktop.".into(),
            "Profile files can be restored into a new or existing Firefox profile while Firefox is closed.".into(),
            "Saved passwords and sign-ins are not migrated; use Firefox Sync or Firefox's own password export.".into(),
        ]
    }
}

pub struct BrowsersModule {
    pub providers: Vec<Arc<dyn BrowserProvider>>,
}

impl Default for BrowsersModule {
    fn default() -> Self {
        Self { providers: default_providers() }
    }
}

pub fn default_providers() -> Vec<Arc<dyn BrowserProvider>> {
    vec![Arc::new(ChromiumProvider::chrome()), Arc::new(ChromiumProvider::edge()), Arc::new(FirefoxProvider)]
}

/// Build discovery items for every profile a provider finds for a user.
pub fn discover_with_provider(
    provider: &dyn BrowserProvider,
    ctx: &DiscoveryContext,
    user: &UserProfile,
    acc: &mut ScanAccumulator,
) -> AppResult<()> {
    let running = ctx.process_running(provider.process_names());
    for root in provider.profile_roots(ctx.platform, user) {
        for (dir, profile_name, path) in provider.enumerate_profiles(&root) {
            let allow = provider.allow_list(ctx.rules.include_cache);
            let label = match &profile_name {
                Some(n) => format!("{} – {} ({dir})", provider.display_name(), n),
                None => format!("{} – {dir}", provider.display_name()),
            };
            let mut item = base_item(
                item_id(&format!("browser-{:?}", provider.kind()).to_lowercase(), Some(user), &format!("{}-{dir}", root.display())),
                Category::Browsers,
                label,
                format!("Browser profile folder \"{dir}\" for {}", user.account_name),
                path_source(&path),
                Some(user),
                RestoreKind::BrowserProfile { browser: provider.kind(), profile_dir: dir.clone() },
                ItemPayload::AllowList { root: path.clone(), files: allow.files, dirs: allow.dirs },
            );
            item.support = SupportLevel::Partial;
            item.includes = provider.supported_components().into_iter().map(String::from).collect();
            item.excludes = provider.excluded_components().into_iter().map(String::from).collect();
            item.restore_notes = provider.restore_notes();
            item.warnings.push(Warning::info(
                WarningCode::CredentialsNotMigrated,
                "Saved passwords, cookies, sign-ins and payment data are protected by Windows and the browser and are never copied. Use the browser's account sync.",
            ));
            if let Some(w) = provider.preflight(&running) {
                item.warnings.push(w);
            }
            measure(&mut item, ctx)?;
            apply_default_selection(&mut item, Some(user));
            acc.browser_profiles.push(BrowserProfileInfo {
                browser: provider.kind(),
                owner: user.user_ref(),
                profile_dir: dir,
                profile_name,
                path,
                size_bytes: item.estimated_size,
            });
            acc.items.push(item);
        }
    }
    Ok(())
}

impl DiscoveryModule for BrowsersModule {
    fn id(&self) -> &'static str {
        "browsers"
    }
    fn stage(&self) -> &'static str {
        "Discovering browser profiles"
    }
    fn discover(&self, ctx: &DiscoveryContext, users: &[UserProfile], acc: &mut ScanAccumulator) -> AppResult<()> {
        for user in eligible_users(users) {
            for p in &self.providers {
                discover_with_provider(p.as_ref(), ctx, user, acc)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_profiles_ini() {
        let ini = "[General]\nStartWithLastProfile=1\n\n[Profile1]\nName=work\nIsRelative=1\nPath=Profiles/abcd.work\n\n[Profile0]\nName=default-release\nIsRelative=1\nPath=Profiles\\xyz.default-release\nDefault=1\n\n[Install308046B0AF4A39CB]\nDefault=Profiles/xyz.default-release\n";
        let v = parse_profiles_ini(ini, Path::new("/ff"));
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].0, "abcd.work");
        assert_eq!(v[0].1.as_deref(), Some("work"));
        assert_eq!(v[1].2, Path::new("/ff/Profiles/xyz.default-release"));
    }

    #[test]
    fn allow_lists_never_contain_secrets() {
        let rules = crate::security::exclusions::ExclusionRules::new(vec![]);
        for p in [&ChromiumProvider::chrome() as &dyn BrowserProvider, &FirefoxProvider] {
            let a = p.allow_list(true);
            for f in &a.files {
                let c = rules.classify(Path::new(f), false);
                assert!(!matches!(c, Some(crate::security::exclusions::ExclusionReason::Sensitive(_))), "{f} is sensitive");
            }
            let lower: Vec<String> = a.files.iter().map(|f| f.to_lowercase()).collect();
            for secret in ["login data", "cookies", "web data", "local state", "logins.json", "key4.db", "cookies.sqlite", "secure preferences"] {
                assert!(!lower.contains(&secret.to_string()), "{secret} must not be allow-listed");
            }
        }
    }
}
