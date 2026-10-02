//! File restore primitives: collision policies, decrypting copy with
//! verification, and timestamp preservation. Existing destination data is
//! never deleted: "replace" first renames the existing file to a .bak copy.

use crate::capture::copier::{is_lock_error, partial_path};
use crate::capture::hashing::HashingWriter;
use crate::error::{AppError, AppResult};
use crate::models::CollisionPolicy;
use crate::security::encryption::{self, BundleKey};
use crate::util::CancelToken;
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// Write to this path (no conflict, or renamed incoming).
    Write(PathBuf),
    /// Keep the existing file; nothing is written.
    Skip,
    /// Rename the existing file to `backup`, then write `target`.
    Replace { target: PathBuf, backup: PathBuf },
}

/// "Report.docx" -> "Report (migrated).docx", "Report (migrated 2).docx", ...
pub fn renamed_candidate(target: &Path, n: u32) -> PathBuf {
    let stem = target.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let ext = target.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let suffix = if n <= 1 { " (migrated)".to_string() } else { format!(" (migrated {n})") };
    target.with_file_name(format!("{stem}{suffix}{ext}"))
}

pub fn backup_path(target: &Path, stamp: &str) -> PathBuf {
    let name = target.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let mut candidate = target.with_file_name(format!("{name}.pre-migration-{stamp}.bak"));
    let mut n = 2;
    while candidate.exists() {
        candidate = target.with_file_name(format!("{name}.pre-migration-{stamp}-{n}.bak"));
        n += 1;
    }
    candidate
}

/// Decide what to do with one incoming file. `replace_confirmed` must be true
/// for [`CollisionPolicy::ReplaceAfterConfirmation`] to take effect.
pub fn resolve(target: &Path, policy: CollisionPolicy, replace_confirmed: bool, stamp: &str) -> AppResult<Resolution> {
    if !target.exists() {
        return Ok(Resolution::Write(target.to_path_buf()));
    }
    match policy {
        CollisionPolicy::SkipExisting => Ok(Resolution::Skip),
        CollisionPolicy::RenameIncoming => {
            for n in 1..10_000 {
                let c = renamed_candidate(target, n);
                if !c.exists() {
                    return Ok(Resolution::Write(c));
                }
            }
            Err(AppError::InvalidRequest(format!("no free name for {}", target.display())))
        }
        CollisionPolicy::ReplaceAfterConfirmation => {
            if !replace_confirmed {
                return Err(AppError::ConfirmationRequired("replacing existing files requires explicit confirmation for this category".into()));
            }
            if target.is_dir() {
                return Err(AppError::InvalidRequest(format!("{} is a folder; folders are never replaced", target.display())));
            }
            Ok(Resolution::Replace { target: target.to_path_buf(), backup: backup_path(target, stamp) })
        }
    }
}

pub struct WriteOutcome {
    pub plain_hash: String,
    pub bytes: u64,
}

/// Copy (and decrypt if needed) a bundle file to `dest` through a temp file.
/// Returns the SHA-256 of the written plaintext.
/// When `expected` is given, the plaintext hash must match before the file is
/// moved into place; otherwise nothing is written.
pub fn write_file(
    src: &Path,
    dest: &Path,
    key: Option<&BundleKey>,
    expected: Option<&str>,
    cancel: &CancelToken,
    on_bytes: &mut dyn FnMut(u64),
) -> AppResult<WriteOutcome> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }
    let tmp = partial_path(dest);
    let result: AppResult<WriteOutcome> = (|| {
        let input = std::fs::File::open(src).map_err(|e| AppError::io(src, e))?;
        let out = std::fs::File::create(&tmp).map_err(|e| AppError::io(&tmp, e))?;
        let mut w = HashingWriter::new(BufWriter::with_capacity(1024 * 1024, out));
        match key {
            Some(k) => {
                cancel.check()?;
                encryption::decrypt_stream(k, input, &mut w, |c| on_bytes(c.len() as u64))?;
            }
            None => {
                let mut r = input;
                let mut buf = vec![0u8; 1024 * 1024];
                loop {
                    cancel.check()?;
                    let n = r.read(&mut buf).map_err(|e| AppError::io(src, e))?;
                    if n == 0 {
                        break;
                    }
                    w.write_all(&buf[..n]).map_err(|e| AppError::io(&tmp, e))?;
                    on_bytes(n as u64);
                }
            }
        }
        let (buf, hash, bytes) = w.finish();
        let f = buf.into_inner().map_err(|e| AppError::io(&tmp, e.into_error()))?;
        f.sync_all().map_err(|e| AppError::io(&tmp, e))?;
        if let Some(exp) = expected {
            if !exp.eq_ignore_ascii_case(&hash) {
                return Err(AppError::Integrity(format!("{} does not match its recorded hash", src.display())));
            }
        }
        Ok(WriteOutcome { plain_hash: hash, bytes })
    })();
    match result {
        Ok(o) => {
            // `dest` was checked to be free (or moved aside) by `resolve`; a
            // file appearing in the meantime is never overwritten.
            if dest.exists() {
                let _ = std::fs::remove_file(&tmp);
                return Err(AppError::InvalidRequest(format!("{} appeared during restore; left untouched", dest.display())));
            }
            std::fs::rename(&tmp, dest).map_err(|e| AppError::io(dest, e))?;
            if let Ok(m) = std::fs::metadata(src).and_then(|m| m.modified()) {
                let _ = filetime::set_file_mtime(dest, filetime::FileTime::from_system_time(m));
            }
            Ok(o)
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

pub fn is_locked(e: &AppError) -> bool {
    matches!(e, AppError::Io { source, .. } if is_lock_error(source))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collision_policies() {
        let d = tempfile::tempdir().unwrap();
        let t = d.path().join("Report.docx");
        assert_eq!(resolve(&t, CollisionPolicy::SkipExisting, false, "s").unwrap(), Resolution::Write(t.clone()));
        std::fs::write(&t, b"existing").unwrap();
        assert_eq!(resolve(&t, CollisionPolicy::SkipExisting, false, "s").unwrap(), Resolution::Skip);
        assert_eq!(resolve(&t, CollisionPolicy::RenameIncoming, false, "s").unwrap(), Resolution::Write(d.path().join("Report (migrated).docx")));
        std::fs::write(d.path().join("Report (migrated).docx"), b"x").unwrap();
        assert_eq!(resolve(&t, CollisionPolicy::RenameIncoming, false, "s").unwrap(), Resolution::Write(d.path().join("Report (migrated 2).docx")));
        assert!(matches!(resolve(&t, CollisionPolicy::ReplaceAfterConfirmation, false, "s"), Err(AppError::ConfirmationRequired(_))));
        match resolve(&t, CollisionPolicy::ReplaceAfterConfirmation, true, "20240101").unwrap() {
            Resolution::Replace { backup, .. } => assert_eq!(backup.file_name().unwrap(), "Report.docx.pre-migration-20240101.bak"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn renamed_candidate_without_extension() {
        assert_eq!(renamed_candidate(Path::new("/x/Makefile"), 1), Path::new("/x/Makefile (migrated)"));
    }

    #[test]
    fn write_file_refuses_to_overwrite() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("src.txt");
        std::fs::write(&src, b"new").unwrap();
        let dest = d.path().join("dest.txt");
        std::fs::write(&dest, b"old").unwrap();
        assert!(write_file(&src, &dest, None, None, &CancelToken::new(), &mut |_| {}).is_err());
        assert_eq!(std::fs::read(&dest).unwrap(), b"old");
        let dest2 = d.path().join("sub/dest2.txt");
        assert!(write_file(&src, &dest2, None, Some(&"0".repeat(64)), &CancelToken::new(), &mut |_| {}).is_err());
        assert!(!dest2.exists(), "a file failing verification is never moved into place");
        let o = write_file(&src, &dest2, None, Some(&crate::capture::hashing::sha256_bytes(b"new")), &CancelToken::new(), &mut |_| {}).unwrap();
        assert_eq!(o.bytes, 3);
        assert_eq!(std::fs::read(&dest2).unwrap(), b"new");
    }
}
