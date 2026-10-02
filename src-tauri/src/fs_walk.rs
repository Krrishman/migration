//! Safe, cancelable directory walker shared by the size scanner and the
//! capture engine.
//!
//! * Never follows symlinks/junctions (no reparse loops, no scope escape).
//! * Never opens file content (cloud placeholders are not hydrated).
//! * Access-denied and other per-entry errors are reported, not fatal.
//! * Applies [`ExclusionRules`] to every directory and file.

use crate::security::exclusions::{ExclusionReason, ExclusionRules};
use crate::security::safe_path::{entry_flags, EntryFlags};
use crate::util::CancelToken;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum WalkEvent<'a> {
    File { path: &'a Path, rel: &'a str, size: u64, modified: Option<std::time::SystemTime>, flags: EntryFlags },
    Excluded { path: &'a Path, reason: ExclusionReason, is_dir: bool },
    LinkSkipped { path: &'a Path },
    Error { path: &'a Path, error: &'a std::io::Error },
}

/// Optional allow-list filter: only these relative files/dirs (case-insensitive)
/// under the root are visited. Used for browser and app-settings captures.
#[derive(Debug, Clone, Default)]
pub struct AllowList {
    pub files: Vec<String>,
    pub dirs: Vec<String>,
}

impl AllowList {
    fn allows_file(&self, rel: &str) -> bool {
        let r = rel.to_lowercase();
        self.files.iter().any(|f| f.to_lowercase() == r) || self.dirs.iter().any(|d| r.starts_with(&format!("{}/", d.to_lowercase())))
    }
    fn allows_dir(&self, rel: &str) -> bool {
        let r = rel.to_lowercase();
        self.dirs.iter().any(|d| {
            let d = d.to_lowercase();
            r == d || r.starts_with(&format!("{d}/")) || d.starts_with(&format!("{r}/"))
        }) || self.files.iter().any(|f| f.to_lowercase().starts_with(&format!("{r}/")))
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct WalkOptions {
    /// Do not descend into subdirectories.
    pub non_recursive: bool,
}

/// Walk `root`, invoking `visit` for every event. Returns `Err(Canceled)` if
/// the token fires. Paths in `rel` use forward slashes.
pub fn walk(
    root: &Path,
    rules: &ExclusionRules,
    allow: Option<&AllowList>,
    opts: WalkOptions,
    cancel: &CancelToken,
    mut visit: impl FnMut(WalkEvent<'_>),
) -> crate::error::AppResult<()> {
    let mut stack: Vec<(PathBuf, String)> = vec![(root.to_path_buf(), String::new())];
    while let Some((dir, rel_dir)) = stack.pop() {
        cancel.check()?;
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(error) => {
                visit(WalkEvent::Error { path: &dir, error: &error });
                continue;
            }
        };
        let mut children: Vec<_> = entries.collect();
        // Deterministic order makes resume and hash lists reproducible.
        children.sort_by_key(|e| e.as_ref().map(|e| e.file_name()).unwrap_or_default());
        for entry in children {
            cancel.check()?;
            let entry = match entry {
                Ok(e) => e,
                Err(error) => {
                    visit(WalkEvent::Error { path: &dir, error: &error });
                    continue;
                }
            };
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            let rel = if rel_dir.is_empty() { name } else { format!("{rel_dir}/{name}") };
            // symlink_metadata: never traverse the link target.
            let meta = match std::fs::symlink_metadata(&path) {
                Ok(m) => m,
                Err(error) => {
                    visit(WalkEvent::Error { path: &path, error: &error });
                    continue;
                }
            };
            let flags = entry_flags(&meta);
            if flags.is_link {
                visit(WalkEvent::LinkSkipped { path: &path });
                continue;
            }
            if meta.is_dir() {
                if let Some(reason) = rules.classify(&path, true) {
                    visit(WalkEvent::Excluded { path: &path, reason, is_dir: true });
                    continue;
                }
                if opts.non_recursive {
                    continue;
                }
                if let Some(a) = allow {
                    if !a.allows_dir(&rel) {
                        continue;
                    }
                }
                stack.push((path, rel));
            } else if meta.is_file() {
                if let Some(a) = allow {
                    if !a.allows_file(&rel) {
                        continue;
                    }
                }
                if let Some(reason) = rules.classify(&path, false) {
                    visit(WalkEvent::Excluded { path: &path, reason, is_dir: false });
                    continue;
                }
                visit(WalkEvent::File { path: &path, rel: &rel, size: meta.len(), modified: meta.modified().ok(), flags });
            }
        }
    }
    Ok(())
}

/// Summary of a size scan.
#[derive(Debug, Default, Clone)]
pub struct SizeSummary {
    pub bytes: u64,
    pub files: u64,
    pub errors: u64,
    pub access_denied: u64,
    pub excluded: u64,
    pub sensitive_excluded: u64,
    pub links_skipped: u64,
    pub cloud_placeholders: u64,
    pub efs_files: u64,
    pub long_paths: u64,
}

/// Cancelable size scanner. Cloud placeholders are counted but their size is
/// not added (their content is not on local disk and will not be captured).
pub fn scan_size(root: &Path, rules: &ExclusionRules, allow: Option<&AllowList>, cancel: &CancelToken) -> crate::error::AppResult<SizeSummary> {
    let mut s = SizeSummary::default();
    walk(root, rules, allow, WalkOptions::default(), cancel, |ev| match ev {
        WalkEvent::File { path, size, flags, .. } => {
            if flags.is_cloud_placeholder || flags.is_offline {
                s.cloud_placeholders += 1;
                return;
            }
            if flags.is_efs_encrypted {
                s.efs_files += 1;
            }
            if crate::security::safe_path::exceeds_max_path(path) {
                s.long_paths += 1;
            }
            s.bytes += size;
            s.files += 1;
        }
        WalkEvent::Excluded { reason, .. } => {
            s.excluded += 1;
            if reason.is_sensitive() {
                s.sensitive_excluded += 1;
            }
        }
        WalkEvent::LinkSkipped { .. } => s.links_skipped += 1,
        WalkEvent::Error { error, .. } => {
            s.errors += 1;
            if error.kind() == std::io::ErrorKind::PermissionDenied {
                s.access_denied += 1;
            }
        }
    })?;
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tree() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        fs::create_dir_all(r.join("Docs/Sub")).unwrap();
        fs::create_dir_all(r.join("Temp")).unwrap();
        fs::create_dir_all(r.join("$Recycle.Bin")).unwrap();
        fs::write(r.join("Docs/a.txt"), b"hello").unwrap();
        fs::write(r.join("Docs/Sub/b.txt"), b"world!").unwrap();
        fs::write(r.join("Temp/junk.bin"), b"xxxxxxxx").unwrap();
        fs::write(r.join("$Recycle.Bin/deleted.txt"), b"xx").unwrap();
        fs::write(r.join("pagefile.sys"), b"xx").unwrap();
        fs::write(r.join("id_rsa"), b"KEY").unwrap();
        d
    }

    #[test]
    fn scans_and_excludes() {
        let d = tree();
        let s = scan_size(d.path(), &ExclusionRules::new(vec![]), None, &CancelToken::new()).unwrap();
        assert_eq!(s.files, 2);
        assert_eq!(s.bytes, 11);
        assert_eq!(s.excluded, 4); // Temp, $Recycle.Bin, pagefile.sys, id_rsa
        assert_eq!(s.sensitive_excluded, 1);
    }

    #[cfg(unix)]
    #[test]
    fn does_not_follow_symlink_loops() {
        let d = tree();
        std::os::unix::fs::symlink(d.path(), d.path().join("Docs/loop")).unwrap();
        let s = scan_size(d.path(), &ExclusionRules::new(vec![]), None, &CancelToken::new()).unwrap();
        assert_eq!(s.links_skipped, 1);
        assert_eq!(s.files, 2);
    }

    #[test]
    fn honours_cancellation() {
        let d = tree();
        let t = CancelToken::new();
        t.cancel();
        assert!(scan_size(d.path(), &ExclusionRules::new(vec![]), None, &t).is_err());
    }

    #[test]
    fn allow_list_limits_scope() {
        let d = tree();
        let allow = AllowList { files: vec!["Docs/a.txt".into()], dirs: vec![] };
        let s = scan_size(d.path(), &ExclusionRules::new(vec![]), Some(&allow), &CancelToken::new()).unwrap();
        assert_eq!(s.files, 1);
        let allow = AllowList { files: vec![], dirs: vec!["docs".into()] };
        let s = scan_size(d.path(), &ExclusionRules::new(vec![]), Some(&allow), &CancelToken::new()).unwrap();
        assert_eq!(s.files, 2);
    }
}
