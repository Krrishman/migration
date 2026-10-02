//! Exclusion rules applied by every file walker. Two families:
//!
//! * **System/noise exclusions** – Windows, Program Files, page/hibernation
//!   files, recycle bins, temp folders and caches.
//! * **Sensitive exclusions** – credential stores, browser password/cookie/
//!   token databases, Wi-Fi profiles and private key containers. These are
//!   *never* captured, regardless of what the technician selects, and the
//!   exclusion is recorded in the manifest.

use super::safe_path::{is_within, normalize_for_compare};
use crate::models::Exclusion;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExclusionReason {
    SystemPath,
    SystemFile,
    RecycleBin,
    Temporary,
    Cache,
    /// Credentials, tokens, cookies, keys. Never configurable.
    Sensitive(&'static str),
}

impl ExclusionReason {
    pub fn describe(self) -> String {
        match self {
            ExclusionReason::SystemPath => "Operating system or program files are never migrated".into(),
            ExclusionReason::SystemFile => "Windows system file (page/hibernation/swap)".into(),
            ExclusionReason::RecycleBin => "Recycle Bin contents".into(),
            ExclusionReason::Temporary => "Temporary files".into(),
            ExclusionReason::Cache => "Cache data (re-created automatically)".into(),
            ExclusionReason::Sensitive(what) => format!("Protected/sensitive data is never copied: {what}"),
        }
    }
    pub fn is_sensitive(self) -> bool {
        matches!(self, ExclusionReason::Sensitive(_))
    }
}

/// File names (case-insensitive) that are always excluded as sensitive.
const SENSITIVE_FILE_NAMES: &[(&str, &str)] = &[
    // Chromium family
    ("login data", "saved browser passwords"),
    ("login data-journal", "saved browser passwords"),
    ("login data for account", "saved browser passwords"),
    ("login data for account-journal", "saved browser passwords"),
    ("cookies", "browser cookies"),
    ("cookies-journal", "browser cookies"),
    ("safe browsing cookies", "browser cookies"),
    ("web data", "autofill and payment data"),
    ("web data-journal", "autofill and payment data"),
    ("account web data", "account tokens"),
    ("local state", "browser encryption key container"),
    ("secure preferences", "machine-bound protected preferences"),
    ("trust tokens", "auth tokens"),
    ("trust tokens-journal", "auth tokens"),
    ("token service", "auth tokens"),
    ("sync data", "account sync tokens"),
    ("affiliation database", "credential affiliation data"),
    ("passkeys", "passkeys"),
    // Firefox
    ("logins.json", "saved browser passwords"),
    ("logins-backup.json", "saved browser passwords"),
    ("key3.db", "browser key database"),
    ("key4.db", "browser key database"),
    ("cert9.db", "certificate/key database"),
    ("cert8.db", "certificate/key database"),
    ("signons.sqlite", "saved browser passwords"),
    ("cookies.sqlite", "browser cookies"),
    ("cookies.sqlite-wal", "browser cookies"),
    ("formhistory.sqlite", "form/autofill history"),
    ("signedinuser.json", "account sign-in token"),
    ("sessionstore.jsonlz4", "session state"),
    ("sessioncheckpoints.json", "session state"),
    ("pkcs11.txt", "security module configuration"),
    // Windows/user-level keys
    ("id_rsa", "private key"),
    ("id_ed25519", "private key"),
    ("id_ecdsa", "private key"),
    ("id_dsa", "private key"),
    ("ntuser.dat", "registry hive (contains protected data)"),
    ("ntuser.dat.log1", "registry hive"),
    ("ntuser.dat.log2", "registry hive"),
    ("usrclass.dat", "registry hive"),
];

/// File extensions excluded as private key or credential material.
const SENSITIVE_EXTENSIONS: &[(&str, &str)] = &[
    ("pfx", "private key/certificate bundle (export via Certificate Manager instead)"),
    ("p12", "private key/certificate bundle (export via Certificate Manager instead)"),
    ("ppk", "private key"),
    ("kdbx", "password manager database (move with the password manager's own export)"),
    ("rdg", "saved remote desktop credentials"),
];

/// Directory names (case-insensitive) excluded as sensitive inside application
/// data (AppData) trees. Dot-folders (`.ssh`, `.gnupg`) are excluded anywhere.
/// Outside AppData these names are ordinary user folders and are kept.
const SENSITIVE_DIR_NAMES: &[(&str, &str)] = &[
    ("sessions", "browser session state"),
    ("session storage", "browser session state"),
    ("local storage", "site storage that may hold tokens"),
    ("indexeddb", "site storage that may hold tokens"),
    ("service worker", "site storage"),
    ("network", "browser network state including cookies"),
    ("webstorage", "site storage"),
    ("storage", "site storage"),
    (".ssh", "SSH private keys"),
    (".gnupg", "GPG private keys"),
];

/// Profile-relative directories (case-insensitive, forward slashes) excluded as sensitive.
const SENSITIVE_PROFILE_DIRS: &[(&str, &str)] = &[
    ("appdata/roaming/microsoft/credentials", "Windows Credential Manager"),
    ("appdata/local/microsoft/credentials", "Windows Credential Manager"),
    ("appdata/roaming/microsoft/protect", "DPAPI master keys"),
    ("appdata/local/microsoft/vault", "Windows Vault"),
    ("appdata/roaming/microsoft/vault", "Windows Vault"),
    ("appdata/roaming/microsoft/crypto", "private key containers"),
    ("appdata/roaming/microsoft/systemcertificates/my", "personal certificates/private keys"),
    ("appdata/local/microsoft/identitycache", "identity tokens"),
    ("appdata/local/microsoft/tokenbroker", "auth tokens"),
    ("appdata/local/packages/microsoft.aad.brokerplugin_cw5n1h2txyewy", "Azure AD tokens"),
];

/// Machine-wide sensitive locations.
const SENSITIVE_SYSTEM_DIRS: &[(&str, &str)] = &[
    ("programdata/microsoft/wlansvc", "Wi-Fi profiles and keys"),
    ("programdata/microsoft/crypto", "machine private keys"),
    ("windows/system32/config", "registry hives"),
];

const SYSTEM_FILE_NAMES: &[&str] = &["pagefile.sys", "hiberfil.sys", "swapfile.sys", "dumpstack.log.tmp"];

const RECYCLE_DIRS: &[&str] = &["$recycle.bin", "recycler", "recycled", "system volume information"];

const TEMP_DIR_NAMES: &[&str] = &["temp", "tmp", "$windows.~bt", "$windows.~ws"];
const TEMP_EXTENSIONS: &[&str] = &["tmp", "crdownload", "partial"];

const CACHE_DIR_NAMES: &[&str] = &[
    "cache",
    "cache2",
    "code cache",
    "gpucache",
    "shadercache",
    "grshadercache",
    "dawncache",
    "dawngraphitecache",
    "dawnwebgpucache",
    "inetcache",
    "startupcache",
    "thumbnails",
    "crashpad",
    "crash reports",
];

#[derive(Debug, Clone)]
pub struct ExclusionRules {
    /// Absolute roots never captured (Windows dir, Program Files ...).
    pub system_roots: Vec<PathBuf>,
    pub include_cache: bool,
}

impl ExclusionRules {
    pub fn new(system_roots: Vec<PathBuf>) -> Self {
        Self { system_roots, include_cache: false }
    }

    pub fn with_cache(mut self, include_cache: bool) -> Self {
        self.include_cache = include_cache;
        self
    }

    /// Whether a root folder may be offered/selected for capture at all.
    pub fn check_root(&self, root: &Path) -> Option<ExclusionReason> {
        if self.system_roots.iter().any(|s| is_within(s, root)) {
            return Some(ExclusionReason::SystemPath);
        }
        // A system root's parent (e.g. C:\ when C:\Windows is blocked) would
        // contain the system tree; refuse it as a whole-folder selection.
        if self.system_roots.iter().any(|s| is_within(root, s)) {
            return Some(ExclusionReason::SystemPath);
        }
        let n = normalize_for_compare(root);
        if let Some((_, what)) = SENSITIVE_SYSTEM_DIRS.iter().find(|(d, _)| n.ends_with(d) || n.contains(&format!("{d}/"))) {
            return Some(ExclusionReason::Sensitive(what));
        }
        if let Some((_, what)) = SENSITIVE_PROFILE_DIRS.iter().find(|(d, _)| n.ends_with(d) || n.contains(&format!("{d}/"))) {
            return Some(ExclusionReason::Sensitive(what));
        }
        None
    }

    /// Classify a single entry encountered while walking.
    pub fn classify(&self, path: &Path, is_dir: bool) -> Option<ExclusionReason> {
        if self.system_roots.iter().any(|s| is_within(s, path)) {
            return Some(ExclusionReason::SystemPath);
        }
        let n = normalize_for_compare(path);
        for (d, what) in SENSITIVE_SYSTEM_DIRS.iter().chain(SENSITIVE_PROFILE_DIRS) {
            if n.ends_with(d) || n.contains(&format!("{d}/")) {
                return Some(ExclusionReason::Sensitive(what));
            }
        }
        let name = n.rsplit('/').next().unwrap_or_default().to_string();
        if is_dir {
            let in_app_data = n.contains("/appdata/");
            if let Some((_, what)) =
                SENSITIVE_DIR_NAMES.iter().find(|(d, _)| *d == name && (in_app_data || d.starts_with('.')))
            {
                return Some(ExclusionReason::Sensitive(what));
            }
            if RECYCLE_DIRS.contains(&name.as_str()) {
                return Some(ExclusionReason::RecycleBin);
            }
            if TEMP_DIR_NAMES.contains(&name.as_str()) {
                return Some(ExclusionReason::Temporary);
            }
            if !self.include_cache && CACHE_DIR_NAMES.contains(&name.as_str()) {
                return Some(ExclusionReason::Cache);
            }
            return None;
        }
        if let Some((_, what)) = SENSITIVE_FILE_NAMES.iter().find(|(f, _)| *f == name) {
            return Some(ExclusionReason::Sensitive(what));
        }
        let ext = name.rsplit_once('.').map(|(_, e)| e.to_string()).unwrap_or_default();
        if let Some((_, what)) = SENSITIVE_EXTENSIONS.iter().find(|(e, _)| *e == ext) {
            return Some(ExclusionReason::Sensitive(what));
        }
        if SYSTEM_FILE_NAMES.contains(&name.as_str()) {
            return Some(ExclusionReason::SystemFile);
        }
        if TEMP_EXTENSIONS.contains(&ext.as_str()) || name.starts_with("~$") {
            return Some(ExclusionReason::Temporary);
        }
        None
    }

    /// Declarative list written into every manifest.
    pub fn manifest_exclusions(&self) -> Vec<Exclusion> {
        let mut v: Vec<Exclusion> = self
            .system_roots
            .iter()
            .map(|r| Exclusion { pattern: r.display().to_string(), reason: ExclusionReason::SystemPath.describe() })
            .collect();
        v.extend(SYSTEM_FILE_NAMES.iter().map(|f| Exclusion { pattern: f.to_string(), reason: ExclusionReason::SystemFile.describe() }));
        v.extend(RECYCLE_DIRS.iter().map(|d| Exclusion { pattern: format!("{d}/"), reason: ExclusionReason::RecycleBin.describe() }));
        v.extend(TEMP_DIR_NAMES.iter().map(|d| Exclusion { pattern: format!("{d}/"), reason: ExclusionReason::Temporary.describe() }));
        if !self.include_cache {
            v.extend(CACHE_DIR_NAMES.iter().map(|d| Exclusion { pattern: format!("{d}/"), reason: ExclusionReason::Cache.describe() }));
        }
        for (f, what) in SENSITIVE_FILE_NAMES {
            v.push(Exclusion { pattern: f.to_string(), reason: ExclusionReason::Sensitive(what).describe() });
        }
        for (e, what) in SENSITIVE_EXTENSIONS {
            v.push(Exclusion { pattern: format!("*.{e}"), reason: ExclusionReason::Sensitive(what).describe() });
        }
        for (d, what) in SENSITIVE_DIR_NAMES.iter().chain(SENSITIVE_PROFILE_DIRS).chain(SENSITIVE_SYSTEM_DIRS) {
            v.push(Exclusion { pattern: format!("{d}/"), reason: ExclusionReason::Sensitive(what).describe() });
        }
        v.push(Exclusion { pattern: "symbolic links and junctions".into(), reason: "Reparse points are never followed (loop and scope protection)".into() });
        v.push(Exclusion { pattern: "cloud-only placeholders".into(), reason: "Online-only files are not downloaded; they stay in the cloud service".into() });
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules() -> ExclusionRules {
        ExclusionRules::new(vec![
            PathBuf::from("C:\\Windows"),
            PathBuf::from("C:\\Program Files"),
            PathBuf::from("C:\\Program Files (x86)"),
        ])
    }

    #[test]
    fn blocks_system_roots_and_their_parents() {
        let r = rules();
        assert_eq!(r.check_root(Path::new("C:\\Windows\\System32")), Some(ExclusionReason::SystemPath));
        assert_eq!(r.check_root(Path::new("C:\\")), Some(ExclusionReason::SystemPath));
        assert_eq!(r.check_root(Path::new("C:\\Program Files (x86)\\App")), Some(ExclusionReason::SystemPath));
        assert_eq!(r.check_root(Path::new("D:\\Projects")), None);
        assert_eq!(r.check_root(Path::new("C:\\Users\\ann\\Documents")), None);
    }

    #[test]
    fn excludes_system_files_recycle_temp_and_cache() {
        let r = rules();
        assert_eq!(r.classify(Path::new("D:\\pagefile.sys"), false), Some(ExclusionReason::SystemFile));
        assert_eq!(r.classify(Path::new("C:\\hiberfil.sys"), false), Some(ExclusionReason::SystemFile));
        assert_eq!(r.classify(Path::new("D:\\$Recycle.Bin"), true), Some(ExclusionReason::RecycleBin));
        assert_eq!(r.classify(Path::new("C:\\Users\\a\\AppData\\Local\\Temp"), true), Some(ExclusionReason::Temporary));
        assert_eq!(r.classify(Path::new("x\\Default\\Cache"), true), Some(ExclusionReason::Cache));
        assert_eq!(r.classify(Path::new("x\\~$report.docx"), false), Some(ExclusionReason::Temporary));
        assert_eq!(rules().with_cache(true).classify(Path::new("x\\Default\\Cache"), true), None);
        assert_eq!(r.classify(Path::new("C:\\Users\\a\\Documents\\report.docx"), false), None);
        // Ordinary user folders that share a name with browser internals are kept.
        assert_eq!(r.classify(Path::new("C:\\Users\\a\\Documents\\Network"), true), None);
        assert_eq!(r.classify(Path::new("C:\\Users\\a\\Documents\\Sessions"), true), None);
    }

    #[test]
    fn sensitive_categories_are_always_excluded() {
        let r = rules().with_cache(true);
        let cases: &[(&str, bool)] = &[
            ("Chrome\\User Data\\Default\\Login Data", false),
            ("Chrome\\User Data\\Default\\Cookies", false),
            ("C:\\Users\\a\\AppData\\Local\\Google\\Chrome\\User Data\\Default\\Network", true),
            ("C:\\Users\\a\\AppData\\Local\\Google\\Chrome\\User Data\\Default\\Local Storage", true),
            ("Chrome\\User Data\\Default\\Web Data", false),
            ("Chrome\\User Data\\Local State", false),
            ("Edge\\User Data\\Default\\Login Data For Account", false),
            ("Firefox\\Profiles\\x.default\\logins.json", false),
            ("Firefox\\Profiles\\x.default\\key4.db", false),
            ("Firefox\\Profiles\\x.default\\cookies.sqlite", false),
            ("C:\\Users\\a\\AppData\\Roaming\\Microsoft\\Credentials", true),
            ("C:\\Users\\a\\AppData\\Local\\Microsoft\\Credentials\\ABC", false),
            ("C:\\Users\\a\\AppData\\Roaming\\Microsoft\\Protect\\S-1-5-21\\key", false),
            ("C:\\Users\\a\\AppData\\Local\\Microsoft\\Vault", true),
            ("C:\\ProgramData\\Microsoft\\Wlansvc\\Profiles\\Interfaces\\x.xml", false),
            ("C:\\Users\\a\\AppData\\Roaming\\Microsoft\\Crypto\\RSA\\key", false),
            ("C:\\Users\\a\\.ssh", true),
            ("C:\\Users\\a\\Documents\\id_rsa", false),
            ("C:\\Users\\a\\Documents\\cert.pfx", false),
            ("C:\\Users\\a\\NTUSER.DAT", false),
        ];
        for (p, is_dir) in cases {
            let c = r.classify(Path::new(p), *is_dir);
            assert!(matches!(c, Some(ExclusionReason::Sensitive(_))), "{p} should be sensitive, got {c:?}");
        }
    }

    #[test]
    fn manifest_exclusions_are_explicit() {
        let ex = rules().manifest_exclusions();
        assert!(ex.iter().any(|e| e.pattern == "login data"));
        assert!(ex.iter().any(|e| e.pattern.contains("wlansvc")));
        assert!(ex.iter().any(|e| e.pattern == "pagefile.sys"));
    }
}
