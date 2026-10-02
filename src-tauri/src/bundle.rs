//! Bundle layout, manifest persistence and integrity verification.
//!
//! ```text
//! <destination_root>/migrations/<computer>-<YYYY-MM-DD_HHmmss>-<short-id>/
//!   manifest.json        manifest.sha256      report.html   report.json
//!   summary-report.html  logs/  inventory/  users/  system/  hashes/
//! ```

use crate::capture::hashing::{self, HashEntry};
use crate::error::{AppError, AppResult, IoContext};
use crate::models::*;
use crate::security::safe_path::{join_within, safe_relative, sanitize_dir_name};
use crate::util::{write_bytes_atomic, write_json_atomic, CancelToken};
use std::path::{Path, PathBuf};

pub const MANIFEST_FILE: &str = "manifest.json";
pub const MANIFEST_HASH_FILE: &str = "manifest.sha256";
pub const MIGRATIONS_DIR: &str = "migrations";
pub const SUBDIRS: [&str; 5] = ["logs", "inventory", "users", "system", "hashes"];

#[derive(Debug, Clone)]
pub struct BundleLayout {
    pub root: PathBuf,
}

pub fn bundle_dir_name(computer_name: &str, created_at: chrono::DateTime<chrono::Local>, bundle_id: &str) -> String {
    format!("{}-{}-{}", sanitize_dir_name(computer_name), created_at.format("%Y-%m-%d_%H%M%S"), &bundle_id.replace('-', "")[..8])
}

impl BundleLayout {
    /// Create a new bundle directory. Never reuses an existing directory.
    pub fn create(destination_root: &Path, computer_name: &str, bundle_id: &str) -> AppResult<Self> {
        let name = bundle_dir_name(computer_name, chrono::Local::now(), bundle_id);
        let root = destination_root.join(MIGRATIONS_DIR).join(name);
        if root.exists() {
            return Err(AppError::InvalidRequest(format!("bundle folder already exists: {}", root.display())));
        }
        for d in SUBDIRS {
            std::fs::create_dir_all(root.join(d)).at(root.join(d))?;
        }
        Ok(Self { root })
    }

    /// Open an existing bundle from its folder or its manifest.json path.
    pub fn open(path: &Path) -> AppResult<Self> {
        let root = if path.file_name().is_some_and(|n| n.eq_ignore_ascii_case(MANIFEST_FILE)) {
            path.parent().map(Path::to_path_buf).unwrap_or_default()
        } else {
            path.to_path_buf()
        };
        if !root.join(MANIFEST_FILE).is_file() {
            return Err(AppError::NotFound(format!("no manifest.json in {}", root.display())));
        }
        Ok(Self { root })
    }

    pub fn manifest_path(&self) -> PathBuf {
        self.root.join(MANIFEST_FILE)
    }
    pub fn manifest_hash_path(&self) -> PathBuf {
        self.root.join(MANIFEST_HASH_FILE)
    }
    pub fn logs(&self) -> PathBuf {
        self.root.join("logs")
    }
    pub fn hashes(&self) -> PathBuf {
        self.root.join("hashes")
    }
    pub fn resolve(&self, rel: &str) -> AppResult<PathBuf> {
        join_within(&self.root, rel)
    }
    pub fn request_path(&self) -> PathBuf {
        self.logs().join("capture-request.json")
    }
}

/// Write the manifest atomically and refresh manifest.sha256.
pub fn write_manifest(layout: &BundleLayout, manifest: &Manifest) -> AppResult<()> {
    let bytes = serde_json::to_vec_pretty(manifest)?;
    write_bytes_atomic(&layout.manifest_path(), &bytes)?;
    let line = format!("{}  {}\n", hashing::sha256_bytes(&bytes), MANIFEST_FILE);
    write_bytes_atomic(&layout.manifest_hash_path(), line.as_bytes())
}

/// Read and validate a manifest. Returns the manifest and whether
/// manifest.sha256 matched.
pub fn read_manifest(layout: &BundleLayout) -> AppResult<(Manifest, bool)> {
    let path = layout.manifest_path();
    let bytes = std::fs::read(&path).at(&path)?;
    if bytes.len() > 256 * 1024 * 1024 {
        return Err(AppError::InvalidManifest("manifest is unreasonably large".into()));
    }
    let manifest = parse_manifest(&bytes)?;
    let hash_ok = std::fs::read_to_string(layout.manifest_hash_path())
        .ok()
        .and_then(|s| s.split_whitespace().next().map(str::to_string))
        .is_some_and(|h| h.eq_ignore_ascii_case(&hashing::sha256_bytes(&bytes)));
    Ok((manifest, hash_ok))
}

/// Parse + schema-version check + semantic validation.
pub fn parse_manifest(bytes: &[u8]) -> AppResult<Manifest> {
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|e| AppError::InvalidManifest(format!("not valid JSON: {e}")))?;
    let version = value
        .get("schema_version")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::InvalidManifest("missing schema_version".into()))?
        .to_string();
    let major: u32 = version
        .split('.')
        .next()
        .and_then(|m| m.parse().ok())
        .ok_or_else(|| AppError::InvalidManifest(format!("invalid schema_version {version}")))?;
    if major != SCHEMA_MAJOR {
        return Err(AppError::UnsupportedSchema { found: version, supported: SCHEMA_MAJOR });
    }
    let manifest: Manifest = serde_json::from_value(value).map_err(|e| AppError::InvalidManifest(e.to_string()))?;
    validate_manifest(&manifest)?;
    Ok(manifest)
}

pub fn validate_manifest(m: &Manifest) -> AppResult<()> {
    uuid::Uuid::parse_str(&m.bundle_id).map_err(|_| AppError::InvalidManifest("bundle_id is not a UUID".into()))?;
    if m.source_machine.computer_name.trim().is_empty() {
        return Err(AppError::InvalidManifest("source computer name is empty".into()));
    }
    let mut ids = std::collections::HashSet::new();
    for item in &m.items {
        if !ids.insert(&item.id) {
            return Err(AppError::InvalidManifest(format!("duplicate item id {}", item.id)));
        }
        safe_relative(&item.bundle_path).map_err(|e| AppError::InvalidManifest(format!("item {}: {e}", item.id)))?;
        if let Some(h) = &item.hash_list {
            safe_relative(h).map_err(|e| AppError::InvalidManifest(format!("item {}: {e}", item.id)))?;
            if !h.starts_with("hashes/") {
                return Err(AppError::InvalidManifest(format!("item {}: hash list outside hashes/", item.id)));
            }
        }
        if let Some(d) = &item.hash_list_sha256 {
            if !hashing::is_sha256_hex(d) {
                return Err(AppError::InvalidManifest(format!("item {}: invalid hash list digest", item.id)));
            }
        }
        if let Some(p) = &item.profile_relative {
            if !p.is_empty() {
                safe_relative(p).map_err(|e| AppError::InvalidManifest(format!("item {}: {e}", item.id)))?;
            }
        }
    }
    for u in &m.users {
        safe_relative(&u.bundle_dir).map_err(|e| AppError::InvalidManifest(format!("user {}: {e}", u.account_name)))?;
    }
    if m.encryption.enabled && (m.encryption.algorithm.is_none() || m.encryption.kdf.is_none() || m.encryption.key_check.is_none()) {
        return Err(AppError::InvalidManifest("encryption is enabled but its metadata is incomplete".into()));
    }
    if let Some(r) = &m.integrity.bundle_root_hash {
        if !hashing::is_sha256_hex(r) {
            return Err(AppError::InvalidManifest("invalid bundle root hash".into()));
        }
    }
    Ok(())
}

pub fn write_request(layout: &BundleLayout, req: &CaptureRequest) -> AppResult<()> {
    // `passphrase` is `skip_serializing`, so it can never reach disk.
    write_json_atomic(&layout.request_path(), req)
}

pub fn read_request(layout: &BundleLayout) -> AppResult<CaptureRequest> {
    let p = layout.request_path();
    Ok(serde_json::from_slice(&std::fs::read(&p).at(&p)?)?)
}

/// Verify bundle integrity. With `full` every payload file is re-hashed;
/// otherwise only hash-list digests and file presence are checked.
pub fn verify_bundle(layout: &BundleLayout, manifest: &Manifest, full: bool, cancel: &CancelToken, mut on_file: impl FnMut(&str)) -> AppResult<BundleValidation> {
    let mut v = BundleValidation {
        bundle_path: layout.root.display().to_string(),
        bundle_id: manifest.bundle_id.clone(),
        schema_version: manifest.schema_version.clone(),
        schema_supported: true,
        manifest_hash_ok: true,
        encrypted: manifest.encryption.enabled,
        files_checked: 0,
        mismatches: vec![],
        missing: vec![],
        errors: vec![],
        ok: false,
    };
    let mut digests = Vec::new();
    for item in &manifest.items {
        let (Some(list_rel), Some(expected)) = (&item.hash_list, &item.hash_list_sha256) else {
            if matches!(item.capture_status, CaptureStatus::Captured | CaptureStatus::CapturedWithWarnings) && item.captured_files > 0 {
                v.errors.push(format!("{}: captured files but no hash list recorded", item.display_name));
            }
            continue;
        };
        digests.push(expected.clone());
        let list_path = layout.resolve(list_rel)?;
        let text = match std::fs::read_to_string(&list_path) {
            Ok(t) => t,
            Err(_) => {
                v.missing.push(list_rel.clone());
                continue;
            }
        };
        if !hashing::sha256_bytes(text.as_bytes()).eq_ignore_ascii_case(expected) {
            v.mismatches.push(list_rel.clone());
            continue;
        }
        let entries: Vec<HashEntry> = match hashing::parse_hash_list(&text) {
            Ok(e) => e,
            Err(e) => {
                v.errors.push(format!("{list_rel}: {e}"));
                continue;
            }
        };
        for e in entries {
            cancel.check()?;
            let p = layout.resolve(&e.path)?;
            on_file(&e.path);
            v.files_checked += 1;
            if !p.is_file() {
                v.missing.push(e.path.clone());
                continue;
            }
            if full {
                match hashing::sha256_file(&p) {
                    Ok(h) if h.eq_ignore_ascii_case(&e.sha256) => {}
                    Ok(_) => v.mismatches.push(e.path.clone()),
                    Err(err) => v.errors.push(format!("{}: {err}", e.path)),
                }
            }
        }
    }
    if let Some(root) = &manifest.integrity.bundle_root_hash {
        let actual = hashing::root_hash(digests.iter().map(String::as_str));
        if !actual.eq_ignore_ascii_case(root) {
            v.errors.push("bundle root hash does not match the recorded module digests".into());
        }
    }
    v.ok = v.mismatches.is_empty() && v.missing.is_empty() && v.errors.is_empty();
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = include_str!("../../SAMPLE_MANIFEST.json");

    #[test]
    fn sample_manifest_matches_schema() {
        let m = parse_manifest(SAMPLE.as_bytes()).unwrap();
        assert_eq!(m.schema_version, SCHEMA_VERSION);
        assert!(!m.items.is_empty());
        // Round-trips without loss of required fields.
        let again = parse_manifest(&serde_json::to_vec(&m).unwrap()).unwrap();
        assert_eq!(again.items.len(), m.items.len());
    }

    #[test]
    fn manifest_never_contains_secret_fields() {
        let v: serde_json::Value = serde_json::from_str(SAMPLE).unwrap();
        fn keys(v: &serde_json::Value, out: &mut Vec<String>) {
            match v {
                serde_json::Value::Object(o) => {
                    for (k, x) in o {
                        out.push(k.to_lowercase());
                        keys(x, out);
                    }
                }
                serde_json::Value::Array(a) => a.iter().for_each(|x| keys(x, out)),
                _ => {}
            }
        }
        let mut k = vec![];
        keys(&v, &mut k);
        for banned in ["password", "passphrase", "token", "cookie", "secret", "private_key", "key"] {
            assert!(!k.iter().any(|x| x == banned), "manifest has a '{banned}' field");
        }
    }

    #[test]
    fn bundle_dir_name_format() {
        let t = chrono::TimeZone::with_ymd_and_hms(&chrono::Local, 2024, 6, 3, 14, 5, 9).unwrap();
        assert_eq!(bundle_dir_name("ACCT PC/07", t, "543b9071-aaaa-bbbb-cccc-000000000000"), "ACCT_PC_07-2024-06-03_140509-543b9071");
    }
}
