//! End-to-end capture tests against the fixture source PC.

mod common;

use common::*;
use migration_assistant_lib::bundle::{self, BundleLayout};
use migration_assistant_lib::capture::CaptureEngine;
use migration_assistant_lib::models::*;
use migration_assistant_lib::progress::{MemorySink, ProgressSink};
use migration_assistant_lib::util::CancelToken;
use std::sync::Arc;

fn engine<'a>(env: &'a Env, sink: Arc<dyn ProgressSink>, cancel: CancelToken) -> CaptureEngine<'a> {
    CaptureEngine { platform: &env.source, sink, cancel }
}

#[test]
fn scan_discovers_expected_items_with_safe_defaults() {
    let env = Env::new();
    let scan = env.scan();
    assert_eq!(scan.machine.computer_name, "ACCT-PC-07");
    assert!(scan.platform.starts_with("fixture:"));
    // System profile listed but never offered; inaccessible profile is flagged.
    assert!(scan.users.iter().any(|u| u.sid == "S-1-5-18" && u.is_system_account));
    assert!(!scan.items.iter().any(|i| i.owner.as_ref().is_some_and(|o| o.sid == "S-1-5-18")));
    let denied = scan.items.iter().find(|i| i.id.contains("inaccessible")).expect("inaccessible profile item");
    assert_eq!(denied.access, AccessState::AccessDenied);
    assert_eq!(denied.support, SupportLevel::Unsupported);

    // Browsers: Chrome Default + Profile 1, Edge Default, Firefox default-release.
    assert_eq!(scan.browser_profiles.len(), 4);
    let chrome = scan.browser_profiles.iter().find(|b| b.profile_dir == "Default" && b.browser == BrowserKind::Chrome).unwrap();
    assert_eq!(chrome.profile_name.as_deref(), Some("Ann (Work)"));
    let edge_item = scan.items.iter().find(|i| matches!(&i.restore_kind, RestoreKind::BrowserProfile { browser: BrowserKind::Edge, .. })).unwrap();
    assert!(edge_item.warnings.iter().any(|w| w.code == WarningCode::BrowserRunning), "msedge.exe is running in the fixture");

    // OST is shown but not capturable; PST is opt-in.
    let ost = scan.items.iter().find(|i| i.display_name.contains(".ost")).unwrap();
    assert_eq!(ost.support, SupportLevel::Unsupported);
    assert!(!ost.selected_by_default);
    let pst = scan.items.iter().find(|i| i.restore_kind == RestoreKind::PstFile).unwrap();
    assert!(pst.opt_in_only && !pst.selected_by_default);

    // Privacy-sensitive and opt-in items are never preselected; other users are not preselected.
    for i in &scan.items {
        if i.sensitive || i.opt_in_only || !i.support.is_capturable() {
            assert!(!i.selected_by_default, "{} must not be preselected", i.display_name);
        }
        if i.owner.as_ref().is_some_and(|o| o.sid != ANN_SID) {
            assert!(!i.selected_by_default, "{} belongs to another user", i.display_name);
        }
    }
    assert!(scan.items.iter().any(|i| i.restore_kind == RestoreKind::RecentItems && i.sensitive));

    // Sizes exclude temp files and secrets: Documents = Report + Budget + plan + PST.
    let docs = scan.items.iter().find(|i| i.id.ends_with(":documents") && i.owner.as_ref().unwrap().sid == ANN_SID).unwrap();
    assert_eq!(docs.item_count, Some(4));
    assert!(docs.warnings.iter().any(|w| w.code == WarningCode::SensitiveExcluded));

    // Inventories and printers.
    assert_eq!(scan.printers.len(), 4);
    assert_eq!(scan.mapped_drives.len(), 2);
    assert_eq!(scan.applications.len(), 7);
    assert!(scan.applications.iter().any(|a| a.display_name == "Contoso Expense Client" && a.description.is_none()));
    assert!(scan.items.iter().any(|i| i.category == Category::ApplicationSettings && i.opt_in_only));
    // Nothing offered from system locations.
    let root = env.source.root().to_path_buf();
    for i in &scan.items {
        if let SourceRef::Path { path } = &i.source {
            for sys in ["Windows", "Program Files", "ProgramData"] {
                assert!(!migration_assistant_lib::security::safe_path::is_within(&root.join(sys), path), "system path offered: {}", path.display());
            }
        }
    }
}

#[test]
fn default_capture_produces_verified_bundle_without_secrets() {
    let env = Env::new();
    let scan = env.scan();
    let sink = sink();
    let req = env.request(default_ids(&scan));
    let summary = engine(&env, sink.clone(), CancelToken::new()).start(&scan, &req).unwrap();
    assert!(summary.verified, "status {:?}, warnings {:?}", summary.status, summary.warnings);
    assert!(matches!(summary.status, BundleStatus::Verified | BundleStatus::VerifiedWithWarnings));

    let layout = BundleLayout::open(std::path::Path::new(&summary.bundle_path)).unwrap();
    let name = layout.root.file_name().unwrap().to_string_lossy().to_string();
    assert!(name.starts_with("ACCT-PC-07-"), "{name}");
    assert_eq!(layout.root.parent().unwrap().file_name().unwrap(), "migrations");
    for f in ["manifest.json", "manifest.sha256", "report.html", "report.json", "summary-report.html", "logs/capture.log.jsonl", "inventory/machine.json", "system/printers.json"] {
        assert!(layout.root.join(f).is_file(), "missing {f}");
    }
    let (manifest, hash_ok) = bundle::read_manifest(&layout).unwrap();
    assert!(hash_ok);
    assert_eq!(manifest.schema_version, SCHEMA_VERSION);
    assert!(manifest.integrity.verified);
    assert!(!manifest.encryption.enabled);
    assert!(manifest.users.iter().any(|u| u.sid == ANN_SID && u.bundle_dir == "users/ann"));
    let v = bundle::verify_bundle(&layout, &manifest, true, &CancelToken::new(), |_| {}).unwrap();
    assert!(v.ok, "{v:?}");
    assert!(v.files_checked > 10);

    // Payload placement and exclusions.
    assert!(layout.root.join("users/ann/files/Documents/Report.docx").is_file());
    assert!(layout.root.join("users/ann/files/Documents/Projects/Migration/plan.txt").is_file());
    assert!(!layout.root.join("users/ann/files/Documents/~$Report.docx").exists());
    assert!(!layout.root.join("users/ann/files/Documents/id_rsa").exists());
    // Timestamps preserved.
    let m = std::fs::metadata(layout.root.join("users/ann/files/Documents/Report.docx")).unwrap().modified().unwrap();
    assert_eq!(m.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(), 1_700_000_000);

    // No secret marker anywhere in the bundle (payload, manifest, reports, logs).
    for f in all_files(&layout.root) {
        let bytes = std::fs::read(&f).unwrap();
        assert!(!contains(&bytes, SECRET_MARKER), "secret leaked into {}", f.display());
    }
    // Progress events: every task reached a terminal state.
    let tasks = sink.tasks.lock();
    assert!(tasks.iter().any(|t| t.state == TaskState::Copying));
    assert!(summary.task_results.iter().all(|t| t.state.is_terminal()));
}

#[test]
fn sensitive_data_is_excluded_even_when_everything_is_selected() {
    let env = Env::new();
    let scan = env.scan();
    let ids: Vec<String> = scan.items.iter().filter(|i| i.support.is_capturable() && i.access != AccessState::AccessDenied).map(|i| i.id.clone()).collect();
    let mut req = env.request(ids);
    req.options.include_browser_cache = true;
    let summary = engine(&env, sink(), CancelToken::new()).start(&scan, &req).unwrap();
    let root = std::path::PathBuf::from(&summary.bundle_path);
    let files = all_files(&root);
    for f in &files {
        let bytes = std::fs::read(f).unwrap();
        assert!(!contains(&bytes, SECRET_MARKER), "secret leaked into {}", f.display());
        let name = f.file_name().unwrap().to_string_lossy().to_lowercase();
        for banned in ["login data", "cookies", "web data", "local state", "logins.json", "key4.db", "cookies.sqlite", "cert9.db", "ntuser.dat", "id_ed25519", "wifi.xml", "sam"] {
            assert_ne!(name, banned, "banned file captured: {}", f.display());
        }
    }
    // Browser bookmarks and Firefox places are captured.
    assert!(files.iter().any(|f| f.ends_with("Bookmarks")));
    assert!(files.iter().any(|f| f.ends_with("places.sqlite")));
    // PST and sticky notes (opt-in) captured when selected.
    assert!(files.iter().any(|f| f.to_string_lossy().ends_with("Archive 2022.pst")));
    assert!(files.iter().any(|f| f.ends_with("plum.sqlite")));
}

/// Cancels the capture in the middle of the Documents task, after at least
/// one of its files has been copied.
struct CancelAfterFirstFile {
    inner: MemorySink,
    cancel: CancelToken,
}
impl ProgressSink for CancelAfterFirstFile {
    fn task(&self, p: &TaskProgress) {
        if p.state == TaskState::Copying && p.items_done >= 1 && p.task_id.ends_with(":documents") {
            self.cancel.cancel();
        }
        self.inner.task(p);
    }
    fn log(&self, e: &LogEntry) {
        self.inner.log(e);
    }
}

#[test]
fn interrupted_capture_resumes_and_verifies() {
    let env = Env::new();
    let scan = env.scan();
    let mut req = env.request(default_ids(&scan));
    // Report.docx is copied after Budget.xlsx; a simulated lock forces a retry
    // delay, so a throttled progress event fires mid-task and triggers cancel.
    env.source.set_locked_files(&["Users/ann/Documents/Report.docx"]);
    req.options.max_retries = 1;
    let cancel = CancelToken::new();
    let s = Arc::new(CancelAfterFirstFile { inner: MemorySink::default(), cancel: cancel.clone() });
    let first = engine(&env, s, cancel).start(&scan, &req).unwrap();
    env.source.set_locked_files(&[]);
    assert_eq!(first.status, BundleStatus::Canceled);
    assert!(!first.verified);
    assert!(first.task_results.iter().any(|t| t.state == TaskState::Canceled));
    let layout = BundleLayout::open(std::path::Path::new(&first.bundle_path)).unwrap();
    let (m, _) = bundle::read_manifest(&layout).unwrap();
    assert_eq!(m.status, BundleStatus::Canceled);
    // No partial temp files are left behind.
    assert!(!all_files(&layout.root).iter().any(|f| f.to_string_lossy().ends_with(".ma-partial")));
    // The checkpoint recorded the file finished before cancellation; it is reused on resume.
    let docs_id = m.items.iter().find(|i| i.id.ends_with(":documents") && i.id.contains(ANN_SID)).unwrap().id.clone();
    let cp = migration_assistant_lib::capture::checkpoint::Checkpoint::open(&layout.logs().join("checkpoint.sqlite")).unwrap();
    let budget = cp.get(&docs_id, "Budget.xlsx").unwrap().expect("Budget.xlsx checkpointed");
    assert_eq!(budget.status, migration_assistant_lib::capture::checkpoint::FileStatus::Done);
    drop(cp);
    let budget_path = layout.root.join("users/ann/files/Documents/Budget.xlsx");
    let ino_before = std::fs::metadata(&budget_path).unwrap().modified().unwrap();

    std::thread::sleep(std::time::Duration::from_millis(20));
    let resumed = engine(&env, sink(), CancelToken::new()).resume(&scan, &layout.root, None).unwrap();
    assert!(resumed.verified, "{:?}", resumed.status);
    assert_eq!(std::fs::metadata(&budget_path).unwrap().modified().unwrap(), ino_before, "finished file was not rewritten");
    assert!(layout.root.join("users/ann/files/Documents/Report.docx").is_file());
    assert_eq!(resumed.bundle_id, first.bundle_id);
    let (m, _) = bundle::read_manifest(&layout).unwrap();
    assert!(bundle::verify_bundle(&layout, &m, true, &CancelToken::new(), |_| {}).unwrap().ok);
    // Resuming a finished bundle is refused.
    assert!(engine(&env, sink(), CancelToken::new()).resume(&scan, &layout.root, None).is_err());
}

#[test]
fn hash_verification_detects_tampering() {
    let env = Env::new();
    let scan = env.scan();
    let summary = engine(&env, sink(), CancelToken::new()).start(&scan, &env.request(default_ids(&scan))).unwrap();
    let layout = BundleLayout::open(std::path::Path::new(&summary.bundle_path)).unwrap();
    let (m, _) = bundle::read_manifest(&layout).unwrap();
    std::fs::write(layout.root.join("users/ann/files/Documents/Budget.xlsx"), b"tampered").unwrap();
    let v = bundle::verify_bundle(&layout, &m, true, &CancelToken::new(), |_| {}).unwrap();
    assert!(!v.ok);
    assert!(v.mismatches.iter().any(|p| p.ends_with("Budget.xlsx")));
    std::fs::remove_file(layout.root.join("users/ann/files/Documents/Report.docx")).unwrap();
    let v = bundle::verify_bundle(&layout, &m, false, &CancelToken::new(), |_| {}).unwrap();
    assert!(v.missing.iter().any(|p| p.ends_with("Report.docx")));
    // Tampering with the manifest breaks manifest.sha256.
    let mut text = std::fs::read_to_string(layout.manifest_path()).unwrap();
    text = text.replace("ACCT-PC-07", "ACCT-PC-99");
    std::fs::write(layout.manifest_path(), text).unwrap();
    let (_, hash_ok) = bundle::read_manifest(&layout).unwrap();
    assert!(!hash_ok);
}

#[test]
fn insufficient_disk_space_blocks_before_writing() {
    let env = Env::new();
    let scan = env.scan();
    env.source.set_free_space(Some(10 * 1024));
    let req = env.request(default_ids(&scan));
    let e = engine(&env, sink(), CancelToken::new());
    let pf = e.preflight(&scan, &req);
    assert!(!pf.sufficient_space);
    assert!(pf.blocking_errors.iter().any(|b| b.contains("Not enough free space")));
    assert!(e.start(&scan, &req).is_err());
    assert!(!env.dest_root.join("migrations").exists(), "nothing may be written when preflight fails");
}

#[test]
fn locked_files_are_retried_then_skipped_with_warning() {
    let env = Env::new();
    let scan = env.scan();
    env.source.set_locked_files(&["Users/ann/Documents/Budget.xlsx"]);
    let mut req = env.request(default_ids(&scan));
    req.options.max_retries = 1;
    let summary = engine(&env, sink(), CancelToken::new()).start(&scan, &req).unwrap();
    let docs = summary.task_results.iter().find(|t| t.task_id.ends_with(":documents") && t.task_id.contains(ANN_SID)).unwrap();
    assert_eq!(docs.state, TaskState::CompletedWithWarnings);
    assert!(docs.retry_count >= 1);
    let layout = BundleLayout::open(std::path::Path::new(&summary.bundle_path)).unwrap();
    let (m, _) = bundle::read_manifest(&layout).unwrap();
    let item = m.items.iter().find(|i| i.id == docs.task_id).unwrap();
    assert_eq!(item.capture_status, CaptureStatus::CapturedWithWarnings);
    assert_eq!(item.skipped_files, 1);
    assert!(item.warnings.iter().any(|w| w.code == WarningCode::LockedFile));
    assert!(!layout.root.join("users/ann/files/Documents/Budget.xlsx").exists());
    // A skipped (not failed) file still allows a verified bundle, with warnings.
    assert_eq!(summary.status, BundleStatus::VerifiedWithWarnings);

    // When skipping is disabled, a locked file is an error and the bundle is not verified.
    let env2 = Env::new();
    let scan2 = env2.scan();
    env2.source.set_locked_files(&["Users/ann/Documents/Budget.xlsx"]);
    let mut req2 = env2.request(default_ids(&scan2));
    req2.options.max_retries = 0;
    req2.options.skip_locked_files = false;
    let s2 = CaptureEngine { platform: &env2.source, sink: sink(), cancel: CancelToken::new() }.start(&scan2, &req2).unwrap();
    assert!(!s2.verified);
    assert_eq!(s2.status, BundleStatus::CompletedUnverified);
}

#[test]
fn destination_inside_captured_folder_is_rejected() {
    let env = Env::new();
    let scan = env.scan();
    let docs = scan.items.iter().find(|i| i.id.ends_with(":documents") && i.owner.as_ref().unwrap().sid == ANN_SID).unwrap();
    let SourceRef::Path { path } = &docs.source else { panic!() };
    let mut req = env.request(vec![docs.id.clone()]);
    req.destination_root = path.display().to_string();
    let pf = engine(&env, sink(), CancelToken::new()).preflight(&scan, &req);
    assert!(pf.blocking_errors.iter().any(|b| b.contains("inside")), "{:?}", pf.blocking_errors);
}

#[test]
fn encrypted_capture_hides_content_and_metadata_holds_no_secret() {
    let env = Env::new();
    let scan = env.scan();
    let mut req = env.request(default_ids(&scan));
    req.encryption = EncryptionRequest { enabled: true, passphrase: Some("correct horse battery staple".into()) };
    let summary = engine(&env, sink(), CancelToken::new()).start(&scan, &req).unwrap();
    assert!(summary.verified);
    let root = std::path::PathBuf::from(&summary.bundle_path);
    assert!(root.join("users/ann/files/Documents/Report.docx.maenc").is_file());
    assert!(!root.join("users/ann/files/Documents/Report.docx").exists());
    let enc = std::fs::read(root.join("users/ann/files/Documents/Report.docx.maenc")).unwrap();
    assert!(!contains(&enc, b"Quarterly report draft"));
    for f in all_files(&root) {
        let b = std::fs::read(&f).unwrap();
        assert!(!contains(&b, b"correct horse battery staple"), "passphrase leaked into {}", f.display());
    }
    let (m, _) = bundle::read_manifest(&BundleLayout::open(&root).unwrap()).unwrap();
    assert!(m.encryption.enabled);
    assert!(m.encryption.kdf.as_ref().unwrap().algorithm.starts_with("argon2id"));
    assert!(root.join("logs/capture-request.json").is_file());
    let reqtext = std::fs::read_to_string(root.join("logs/capture-request.json")).unwrap();
    assert!(!reqtext.contains("passphrase\":\"correct"));
}

#[test]
fn malformed_manifests_are_rejected() {
    let env = Env::new();
    let scan = env.scan();
    let summary = engine(&env, sink(), CancelToken::new()).start(&scan, &env.request(default_ids(&scan))).unwrap();
    let layout = BundleLayout::open(std::path::Path::new(&summary.bundle_path)).unwrap();
    let good: serde_json::Value = serde_json::from_slice(&std::fs::read(layout.manifest_path()).unwrap()).unwrap();
    assert!(bundle::parse_manifest(&serde_json::to_vec(&good).unwrap()).is_ok());

    let mutate = |f: &dyn Fn(&mut serde_json::Value)| {
        let mut v = good.clone();
        f(&mut v);
        bundle::parse_manifest(&serde_json::to_vec(&v).unwrap())
    };
    assert!(bundle::parse_manifest(b"not json").is_err());
    assert!(mutate(&|v| v["schema_version"] = "2.0".into()).is_err());
    assert!(mutate(&|v| { v.as_object_mut().unwrap().remove("schema_version"); }).is_err());
    assert!(mutate(&|v| v["bundle_id"] = "not-a-uuid".into()).is_err());
    assert!(mutate(&|v| v["items"][0]["bundle_path"] = "../../Windows/System32".into()).is_err());
    assert!(mutate(&|v| v["items"][0]["bundle_path"] = "C:\\Windows".into()).is_err());
    assert!(mutate(&|v| v["items"][0]["hash_list"] = "users/x.sha256".into()).is_err());
    assert!(mutate(&|v| v["items"][0]["hash_list_sha256"] = "zz".into()).is_err());
    assert!(mutate(&|v| { v.as_object_mut().unwrap().remove("items"); }).is_err());
    assert!(mutate(&|v| v["encryption"]["enabled"] = true.into()).is_err());
    assert!(mutate(&|v| v["status"] = "exploded".into()).is_err());
}
