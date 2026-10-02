//! Verified single-file copy: retry on locks, write to a temp file, fsync,
//! atomic rename, preserve modification time, optional encryption, and
//! re-read verification of the stored bytes.

use super::hashing::{sha256_file, HashingWriter};
use crate::platform::Platform;
use crate::security::encryption::{self, BundleKey};
use crate::util::CancelToken;
use sha2::{Digest, Sha256};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const PARTIAL_SUFFIX: &str = ".ma-partial";

#[derive(Debug)]
pub struct CopyOutcome {
    pub stored_hash: String,
    pub plain_hash: String,
    pub plain_bytes: u64,
    pub stored_bytes: u64,
    pub retries: u32,
}

#[derive(Debug)]
pub enum CopyError {
    /// Held open by another process after all retries.
    Locked(String),
    AccessDenied(String),
    Canceled,
    /// Stored bytes did not match the hash computed while writing.
    HashMismatch,
    Other(String),
}

impl std::fmt::Display for CopyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CopyError::Locked(e) => write!(f, "file is in use by another process: {e}"),
            CopyError::AccessDenied(e) => write!(f, "access denied: {e}"),
            CopyError::Canceled => write!(f, "canceled"),
            CopyError::HashMismatch => write!(f, "verification failed: stored file does not match"),
            CopyError::Other(e) => write!(f, "{e}"),
        }
    }
}

pub fn is_lock_error(e: &std::io::Error) -> bool {
    // ERROR_SHARING_VIOLATION (32) / ERROR_LOCK_VIOLATION (33) on Windows.
    (cfg!(windows) && matches!(e.raw_os_error(), Some(32) | Some(33))) || e.kind() == std::io::ErrorKind::ResourceBusy
}

fn classify(e: std::io::Error) -> CopyError {
    if is_lock_error(&e) {
        CopyError::Locked(e.to_string())
    } else if e.kind() == std::io::ErrorKind::PermissionDenied {
        CopyError::AccessDenied(e.to_string())
    } else {
        CopyError::Other(e.to_string())
    }
}

/// Reader that aborts promptly when the cancel token fires.
struct CancelReader<R> {
    inner: R,
    cancel: CancelToken,
    on_read: Box<dyn FnMut(usize)>,
}

impl<R: Read> Read for CancelReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.cancel.is_canceled() {
            // Not `Interrupted`: std's read helpers would silently retry that kind.
            return Err(std::io::Error::other("copy canceled"));
        }
        let n = self.inner.read(buf)?;
        (self.on_read)(n);
        Ok(n)
    }
}

pub fn partial_path(dest: &Path) -> PathBuf {
    let mut s = dest.as_os_str().to_os_string();
    s.push(PARTIAL_SUFFIX);
    PathBuf::from(s)
}

pub struct CopySpec<'a> {
    pub src: &'a Path,
    pub dest: &'a Path,
    pub key: Option<&'a BundleKey>,
    pub verify: bool,
    pub max_retries: u32,
    pub cancel: &'a CancelToken,
    pub platform: &'a dyn Platform,
}

/// Copy with retries. `on_bytes` receives plaintext progress (may be called
/// again from zero on a retry; callers track the net).
pub fn copy_verified(spec: &CopySpec, on_bytes: &mut dyn FnMut(u64)) -> Result<CopyOutcome, CopyError> {
    let mut retries = 0;
    let mut mismatch_retry_used = false;
    loop {
        if spec.cancel.is_canceled() {
            return Err(CopyError::Canceled);
        }
        match copy_once(spec, on_bytes) {
            Ok(mut o) => {
                o.retries = retries;
                return Ok(o);
            }
            Err(CopyError::Locked(e)) if retries < spec.max_retries => {
                retries += 1;
                std::thread::sleep(Duration::from_millis(250 * (1 << retries.min(4))));
                let _ = e;
            }
            Err(CopyError::HashMismatch) if !mismatch_retry_used => {
                mismatch_retry_used = true;
                retries += 1;
            }
            Err(e) => {
                let _ = std::fs::remove_file(partial_path(spec.dest));
                return Err(e);
            }
        }
    }
}

fn copy_once(spec: &CopySpec, on_bytes: &mut dyn FnMut(u64)) -> Result<CopyOutcome, CopyError> {
    if let Some(e) = spec.platform.injected_open_error(spec.src) {
        return Err(classify(e));
    }
    let src_file = std::fs::File::open(spec.src).map_err(classify)?;
    let src_meta = src_file.metadata().map_err(classify)?;
    if let Some(parent) = spec.dest.parent() {
        std::fs::create_dir_all(parent).map_err(classify)?;
    }
    let tmp = partial_path(spec.dest);
    let out = std::fs::File::create(&tmp).map_err(classify)?;
    let mut writer = HashingWriter::new(BufWriter::with_capacity(1024 * 1024, out));

    // Progress is reported through a shared counter so the reader closure stays 'static.
    let counter = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let c2 = counter.clone();
    let mut reader = CancelReader {
        inner: src_file,
        cancel: spec.cancel.clone(),
        on_read: Box::new(move |n| {
            c2.fetch_add(n as u64, std::sync::atomic::Ordering::Relaxed);
        }),
    };
    let mut plain_hasher = Sha256::new();
    let mut reported = 0u64;
    let result: Result<(), CopyError> = (|| {
        match spec.key {
            Some(key) => {
                // Encryption reads the whole stream; progress is reported after.
                encryption::encrypt_stream(key, &mut reader, &mut writer, |chunk| plain_hasher.update(chunk)).map_err(|e| match e {
                    _ if spec.cancel.is_canceled() => CopyError::Canceled,
                    crate::error::AppError::Io { source, .. } => classify(source),
                    other => CopyError::Other(other.to_string()),
                })?;
            }
            None => {
                let mut buf = vec![0u8; 1024 * 1024];
                loop {
                    let n = match reader.read(&mut buf) {
                        Ok(n) => n,
                        Err(_) if spec.cancel.is_canceled() => return Err(CopyError::Canceled),
                        Err(e) => return Err(classify(e)),
                    };
                    if n == 0 {
                        break;
                    }
                    plain_hasher.update(&buf[..n]);
                    writer.write_all(&buf[..n]).map_err(classify)?;
                    let now = counter.load(std::sync::atomic::Ordering::Relaxed);
                    on_bytes(now - reported);
                    reported = now;
                }
            }
        }
        Ok(())
    })();
    if let Err(e) = result {
        drop(writer);
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    let total = counter.load(std::sync::atomic::Ordering::Relaxed);
    if total > reported {
        on_bytes(total - reported);
    }
    let (buf, stored_hash, stored_bytes) = writer.finish();
    let file = buf.into_inner().map_err(|e| classify(e.into_error()))?;
    file.sync_all().map_err(classify)?;
    drop(file);
    std::fs::rename(&tmp, spec.dest).map_err(classify)?;
    if let Ok(mtime) = src_meta.modified() {
        let _ = filetime::set_file_mtime(spec.dest, filetime::FileTime::from_system_time(mtime));
    }
    if spec.verify {
        let on_disk = sha256_file(spec.dest).map_err(|e| CopyError::Other(e.to_string()))?;
        if on_disk != stored_hash {
            return Err(CopyError::HashMismatch);
        }
    }
    Ok(CopyOutcome { stored_hash, plain_hash: hex::encode(plain_hasher.finalize()), plain_bytes: total, stored_bytes, retries: 0 })
}
