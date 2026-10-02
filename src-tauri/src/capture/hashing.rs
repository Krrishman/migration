//! SHA-256 hashing helpers and sha256sum-compatible hash lists.
//!
//! Strategy (recorded verbatim in every manifest): every stored payload file
//! is hashed while it is written and re-hashed from disk after writing. Each
//! module gets a `hashes/<task>.sha256` list in `sha256sum` format (so it can
//! be checked with standard tools); the manifest records the SHA-256 of each
//! list, and a bundle root hash over all list digests in item order.

use crate::error::{AppError, AppResult, IoContext};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::Path;

pub const HASH_STRATEGY: &str =
    "per-file SHA-256 of stored bytes (hashed during write and re-read after write); per-module sha256sum lists; manifest records list digests and a bundle root hash over them";

pub fn sha256_bytes(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

pub fn sha256_file(path: &Path) -> AppResult<String> {
    let mut f = std::fs::File::open(path).at(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = f.read(&mut buf).at(path)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

pub fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Writer adapter that hashes everything written through it.
pub struct HashingWriter<W: Write> {
    inner: W,
    hasher: Sha256,
    pub bytes: u64,
}

impl<W: Write> HashingWriter<W> {
    pub fn new(inner: W) -> Self {
        Self { inner, hasher: Sha256::new(), bytes: 0 }
    }
    pub fn finish(self) -> (W, String, u64) {
        (self.inner, hex::encode(self.hasher.finalize()), self.bytes)
    }
}

impl<W: Write> Write for HashingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.hasher.update(&buf[..n]);
        self.bytes += n as u64;
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

/// One line of a hash list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HashEntry {
    pub sha256: String,
    /// Bundle-relative path with forward slashes.
    pub path: String,
}

/// Render a sha256sum-format list (sorted by path for reproducibility).
pub fn render_hash_list(entries: &[HashEntry]) -> String {
    let mut sorted: Vec<&HashEntry> = entries.iter().collect();
    sorted.sort_by(|a, b| a.path.cmp(&b.path));
    let mut s = String::new();
    for e in sorted {
        s.push_str(&e.sha256);
        s.push_str("  ");
        s.push_str(&e.path);
        s.push('\n');
    }
    s
}

pub fn parse_hash_list(text: &str) -> AppResult<Vec<HashEntry>> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let (hash, path) = line.split_once("  ").ok_or_else(|| AppError::Integrity(format!("hash list line {} is malformed", i + 1)))?;
        if !is_sha256_hex(hash) {
            return Err(AppError::Integrity(format!("hash list line {} has an invalid digest", i + 1)));
        }
        // Validate paths at parse time so a tampered list cannot point outside the bundle.
        crate::security::safe_path::safe_relative(path)?;
        out.push(HashEntry { sha256: hash.to_ascii_lowercase(), path: path.to_string() });
    }
    Ok(out)
}

pub fn write_hash_list(path: &Path, entries: &[HashEntry]) -> AppResult<String> {
    let text = render_hash_list(entries);
    crate::util::write_bytes_atomic(path, text.as_bytes())?;
    Ok(sha256_bytes(text.as_bytes()))
}

/// Root hash over module list digests, in the given order.
pub fn root_hash<'a>(digests: impl IntoIterator<Item = &'a str>) -> String {
    let mut h = Sha256::new();
    for d in digests {
        h.update(d.as_bytes());
        h.update(b"\n");
    }
    hex::encode(h.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_vector() {
        assert_eq!(sha256_bytes(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn hash_list_roundtrip_and_validation() {
        let entries = vec![
            HashEntry { sha256: sha256_bytes(b"b"), path: "users/a/files/b.txt".into() },
            HashEntry { sha256: sha256_bytes(b"a"), path: "users/a/files/a.txt".into() },
        ];
        let text = render_hash_list(&entries);
        assert!(text.starts_with(&sha256_bytes(b"a")));
        let parsed = parse_hash_list(&text).unwrap();
        assert_eq!(parsed.len(), 2);
        assert!(parse_hash_list("nothex  a.txt\n").is_err());
        assert!(parse_hash_list(&format!("{}  ../../etc/passwd\n", sha256_bytes(b"x"))).is_err());
        assert!(parse_hash_list("no-separator\n").is_err());
    }

    #[test]
    fn hashing_writer_matches() {
        let mut w = HashingWriter::new(Vec::new());
        w.write_all(b"abc").unwrap();
        let (buf, h, n) = w.finish();
        assert_eq!(buf, b"abc");
        assert_eq!(n, 3);
        assert_eq!(h, sha256_bytes(b"abc"));
    }
}
