//! Shared helpers for integration tests: copy a fixture PC into a temp dir,
//! load the fixture platform adapter, scan, and capture.

#![allow(dead_code)]

use migration_assistant_lib::discovery::DiscoveryService;
use migration_assistant_lib::models::*;
use migration_assistant_lib::platform::fixture::FixturePlatform;
use migration_assistant_lib::platform::Platform;
use migration_assistant_lib::progress::MemorySink;
use migration_assistant_lib::util::CancelToken;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const SECRET_MARKER: &[u8] = b"FIXTURE-SECRET-DO-NOT-COPY";
pub const ANN_SID: &str = "S-1-5-21-1004336348-1177238915-682003330-1104";
pub const TARGET_SID: &str = "S-1-12-1-3456789012-1234567890-987654321-1001";

pub fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("fixtures")
}

pub fn copy_tree(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for e in std::fs::read_dir(src).unwrap() {
        let e = e.unwrap();
        let to = dst.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_tree(&e.path(), &to);
        } else {
            std::fs::copy(e.path(), &to).unwrap();
            let m = std::fs::metadata(e.path()).unwrap().modified().unwrap();
            filetime::set_file_mtime(&to, filetime::FileTime::from_system_time(m)).unwrap();
        }
    }
}

pub struct Env {
    pub temp: tempfile::TempDir,
    pub source: FixturePlatform,
    pub dest_root: PathBuf,
}

impl Env {
    pub fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let src = temp.path().join("source-pc");
        copy_tree(&fixtures_dir().join("source-pc"), &src);
        let dest_root = temp.path().join("usb");
        std::fs::create_dir_all(&dest_root).unwrap();
        let source = FixturePlatform::load(&src).unwrap();
        Self { temp, source, dest_root }
    }

    pub fn target(&self) -> FixturePlatform {
        let dst = self.temp.path().join("target-pc");
        if !dst.exists() {
            copy_tree(&fixtures_dir().join("target-pc"), &dst);
        }
        FixturePlatform::load(&dst).unwrap()
    }

    pub fn scan(&self) -> ScanResult {
        DiscoveryService::new().scan(&self.source, &CancelToken::new(), true, &|_| {}).unwrap()
    }

    pub fn request(&self, ids: Vec<String>) -> CaptureRequest {
        CaptureRequest {
            destination_root: self.dest_root.display().to_string(),
            selected_item_ids: ids,
            encryption: EncryptionRequest::default(),
            options: CaptureOptions::default(),
        }
    }
}

pub fn default_ids(scan: &ScanResult) -> Vec<String> {
    scan.items.iter().filter(|i| i.selected_by_default).map(|i| i.id.clone()).collect()
}

pub fn sink() -> Arc<MemorySink> {
    Arc::new(MemorySink::default())
}

/// Every file under `dir`, recursively.
pub fn all_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(all_files(&p));
            } else {
                out.push(p);
            }
        }
    }
    out
}

pub fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

pub fn platform_name(p: &dyn Platform) -> String {
    p.name()
}
