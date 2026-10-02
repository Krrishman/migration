//! SafePath: path canonicalization, traversal prevention, sanitized names,
//! system-root blocking and reparse-point detection.
//!
//! Comparisons are done on a normalized form (forward slashes, lower case,
//! verbatim prefix removed) so the same rules apply to real Windows paths and
//! to fixture trees used in tests on any OS.

use crate::error::{AppError, AppResult};
use std::path::{Component, Path, PathBuf};

/// Windows reserved device names that cannot be used as file names.
const RESERVED_NAMES: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8", "com9",
    "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

pub const MAX_PATH_CLASSIC: usize = 260;

fn clean(path: &Path) -> String {
    let mut s = path.to_string_lossy().replace('\\', "/");
    for prefix in ["//?/UNC/", "//?/", "//./"] {
        if let Some(rest) = s.strip_prefix(prefix) {
            s = if prefix.ends_with("UNC/") { format!("//{rest}") } else { rest.to_string() };
            break;
        }
    }
    while s.len() > 1 && s.ends_with('/') && !s.ends_with(":/") {
        s.pop();
    }
    s
}

/// Normalize a path for comparisons. Never used for I/O.
pub fn normalize_for_compare(path: &Path) -> String {
    clean(path).to_lowercase()
}

fn parts(s: &str) -> Vec<&str> {
    s.split('/').filter(|p| !p.is_empty() && *p != ".").collect()
}

/// True when `path` equals `base` or lies underneath it (component-wise,
/// case-insensitive).
pub fn is_within(base: &Path, path: &Path) -> bool {
    let b = normalize_for_compare(base);
    let p = normalize_for_compare(path);
    let (bp, pp) = (parts(&b), parts(&p));
    // UNC roots and absolute roots must agree too.
    b.starts_with("//") == p.starts_with("//") && bp.len() <= pp.len() && bp.iter().zip(&pp).all(|(x, y)| x == y)
}

/// Relative path of `path` under `base` (forward slashes, original casing).
pub fn relative_to(base: &Path, path: &Path) -> Option<String> {
    if !is_within(base, path) {
        return None;
    }
    let b = clean(base);
    let p = clean(path);
    let n = parts(&b).len();
    Some(parts(&p)[n..].join("/"))
}

/// Strip the `\\?\` prefix that `std::fs::canonicalize` adds on Windows so
/// paths stay readable in the UI and reports.
pub fn display_path(path: &Path) -> String {
    let s = path.display().to_string();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{rest}");
    }
    s.strip_prefix(r"\\?\").map(str::to_string).unwrap_or(s)
}

/// Canonicalize an existing path. Fails if it does not exist.
pub fn canonicalize(path: &Path) -> AppResult<PathBuf> {
    let c = std::fs::canonicalize(path).map_err(|e| AppError::io(path, e))?;
    Ok(PathBuf::from(display_path(&c)))
}

/// Make a single path component safe for Windows file systems.
pub fn sanitize_component(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if (c as u32) < 32 => '_',
            c => c,
        })
        .collect();
    while out.ends_with('.') || out.ends_with(' ') {
        out.pop();
    }
    let out = out.trim_start().to_string();
    let mut out = if out.is_empty() || out == "." || out == ".." { "unnamed".to_string() } else { out };
    let stem = out.split('.').next().unwrap_or("").to_lowercase();
    if RESERVED_NAMES.contains(&stem.as_str()) {
        out = format!("_{out}");
    }
    if out.chars().count() > 80 {
        out = out.chars().take(80).collect();
    }
    out
}

/// Sanitize a computer or account name for use in a bundle folder name.
pub fn sanitize_dir_name(name: &str) -> String {
    let s: String = sanitize_component(name)
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '_' })
        .collect();
    let s = s.trim_matches('.').to_string();
    if s.is_empty() { "unnamed".into() } else { s }
}

/// Parse a bundle-relative path coming from a manifest or the UI. Rejects
/// absolute paths, drive letters, UNC prefixes, `..`, alternate data streams
/// and empty components. Returns a relative `PathBuf` safe to join.
pub fn safe_relative(rel: &str) -> AppResult<PathBuf> {
    if rel.is_empty() {
        return Err(AppError::UnsafePath("empty relative path".into()));
    }
    if rel.starts_with('/') || rel.starts_with('\\') {
        return Err(AppError::UnsafePath(format!("absolute path not allowed: {rel}")));
    }
    let mut out = PathBuf::new();
    for part in rel.split(['/', '\\']) {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return Err(AppError::UnsafePath(format!("parent traversal not allowed: {rel}")));
        }
        if part.contains(':') {
            return Err(AppError::UnsafePath(format!("drive or stream syntax not allowed: {rel}")));
        }
        if part.chars().any(|c| (c as u32) < 32) {
            return Err(AppError::UnsafePath(format!("control characters not allowed: {rel}")));
        }
        out.push(part);
    }
    if out.as_os_str().is_empty() {
        return Err(AppError::UnsafePath(format!("path has no components: {rel}")));
    }
    // Defence in depth: the result must only contain normal components.
    if out.components().any(|c| !matches!(c, Component::Normal(_))) {
        return Err(AppError::UnsafePath(format!("unexpected path component in {rel}")));
    }
    Ok(out)
}

/// Join a bundle-relative path to `base`, guaranteeing the result stays inside.
pub fn join_within(base: &Path, rel: &str) -> AppResult<PathBuf> {
    let joined = base.join(safe_relative(rel)?);
    if !is_within(base, &joined) {
        return Err(AppError::UnsafePath(format!("{rel} escapes {}", base.display())));
    }
    Ok(joined)
}

/// Facts about a directory entry relevant to safe traversal.
#[derive(Debug, Clone, Copy, Default)]
pub struct EntryFlags {
    /// Symlink or junction (name-surrogate reparse point). Never followed.
    pub is_link: bool,
    /// Cloud-files placeholder whose content is not on local disk.
    pub is_cloud_placeholder: bool,
    /// EFS-encrypted file; copying may fail or produce an unreadable copy on another account.
    pub is_efs_encrypted: bool,
    pub is_offline: bool,
}

/// Inspect entry flags without following links and without opening file
/// content (so cloud placeholders are never hydrated/downloaded).
pub fn entry_flags(meta: &std::fs::Metadata) -> EntryFlags {
    let is_link = meta.file_type().is_symlink();
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_OFFLINE: u32 = 0x1000;
        const FILE_ATTRIBUTE_ENCRYPTED: u32 = 0x4000;
        const FILE_ATTRIBUTE_RECALL_ON_OPEN: u32 = 0x40000;
        const FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS: u32 = 0x400000;
        let a = meta.file_attributes();
        EntryFlags {
            is_link,
            is_cloud_placeholder: a & (FILE_ATTRIBUTE_RECALL_ON_OPEN | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS) != 0,
            is_efs_encrypted: a & FILE_ATTRIBUTE_ENCRYPTED != 0,
            is_offline: a & FILE_ATTRIBUTE_OFFLINE != 0,
        }
    }
    #[cfg(not(windows))]
    {
        EntryFlags { is_link, ..Default::default() }
    }
}

/// True if a path would exceed the classic MAX_PATH limit.
pub fn exceeds_max_path(path: &Path) -> bool {
    display_path(path).chars().count() >= MAX_PATH_CLASSIC
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_reserved_and_invalid_names() {
        assert_eq!(sanitize_component("CON"), "_CON");
        assert_eq!(sanitize_component("nul.txt"), "_nul.txt");
        assert_eq!(sanitize_component("a<b>c:d"), "a_b_c_d");
        assert_eq!(sanitize_component("trailing. . "), "trailing");
        assert_eq!(sanitize_component(".."), "unnamed");
        assert_eq!(sanitize_component(""), "unnamed");
        assert_eq!(sanitize_dir_name("DESKTOP 01/ünï"), "DESKTOP_01__n_");
    }

    #[test]
    fn rejects_traversal_and_absolute_paths() {
        assert!(safe_relative("../x").is_err());
        assert!(safe_relative("a/../../x").is_err());
        assert!(safe_relative("/etc/passwd").is_err());
        assert!(safe_relative("\\\\server\\share").is_err());
        assert!(safe_relative("C:\\Windows").is_err());
        assert!(safe_relative("file.txt:stream").is_err());
        assert!(safe_relative("").is_err());
        assert_eq!(safe_relative("users/jdoe/files").unwrap(), PathBuf::from("users").join("jdoe").join("files"));
        assert_eq!(safe_relative("a\\b").unwrap(), PathBuf::from("a").join("b"));
    }

    #[test]
    fn join_within_stays_inside() {
        let base = Path::new("/bundle");
        assert!(join_within(base, "users/a.txt").is_ok());
        assert!(join_within(base, "../a.txt").is_err());
    }

    #[test]
    fn within_is_component_aware_and_case_insensitive() {
        assert!(is_within(Path::new("C:\\Users\\Ann"), Path::new("c:/users/ann/Documents")));
        assert!(!is_within(Path::new("C:\\Users\\Ann"), Path::new("C:\\Users\\Anna\\Documents")));
        assert!(is_within(Path::new("C:\\Windows"), Path::new("\\\\?\\C:\\Windows\\System32")));
    }

    #[test]
    fn relative_to_keeps_original_case() {
        let r = relative_to(Path::new("C:\\Users\\Ann"), Path::new("C:\\Users\\Ann\\AppData\\Roaming")).unwrap();
        assert_eq!(r, "AppData/Roaming");
        assert!(relative_to(Path::new("/a/b"), Path::new("/a/c")).is_none());
    }

    #[test]
    fn display_strips_verbatim_prefix() {
        assert_eq!(display_path(Path::new(r"\\?\C:\Users")), r"C:\Users");
        assert_eq!(display_path(Path::new(r"\\?\UNC\srv\share")), r"\\srv\share");
    }
}
