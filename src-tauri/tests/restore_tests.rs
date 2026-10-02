//! End-to-end restore tests: capture on the fixture source PC, restore onto
//! the fixture target PC.

mod common;

use common::*;
use migration_assistant_lib::capture::CaptureEngine;
use migration_assistant_lib::models::*;
use migration_assistant_lib::platform::fixture::FixturePlatform;
use migration_assistant_lib::restore::{default_mappings, RestoreService};
use migration_assistant_lib::util::CancelToken;
use std::path::{Path, PathBuf};

struct Captured {
    env: Env,
    bundle: PathBuf,
    scan: ScanResult,
}

fn capture(extra: &[&str], passphrase: Option<&str>) -> Captured {
    let env = Env::new();
    let scan = env.scan();
    let mut ids = default_ids(&scan);
    for i in &scan.items {
        if extra.iter().any(|e| i.id.contains(e)) && !ids.contains(&i.id) && i.support.is_capturable() {
            ids.push(i.id.clone());
        }
    }
    let mut req = env.request(ids);
    if let Some(p) = passphrase {
        req.encryption = EncryptionRequest { enabled: true, passphrase: Some(p.into()) };
    }
    let s = CaptureEngine { platform: &env.source, sink: sink(), cancel: CancelToken::new() }.start(&scan, &req).unwrap();
    assert!(s.verified);
    Captured { bundle: PathBuf::from(&s.bundle_path), env, scan }
}

fn service(t: &FixturePlatform) -> RestoreService<'_> {
    RestoreService { platform: t, sink: sink(), cancel: CancelToken::new() }
}

fn request(opened: &migration_assistant_lib::restore::OpenedBundle, target: &FixturePlatform) -> RestoreRequest {
    let profiles = migration_assistant_lib::platform::Platform::list_profiles(target).unwrap();
    RestoreRequest {
        bundle_path: opened.layout.root.display().to_string(),
        mappings: default_mappings(&opened.manifest, &profiles),
        selected_item_ids: opened.manifest.items.iter().map(|i| i.id.clone()).collect(),
        policies: Default::default(),
        replace_confirmed: vec![],
        confirmed_categories: Category::ALL.to_vec(),
        passphrase: None,
    }
}

fn snapshot(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut v: Vec<_> = all_files(dir).into_iter().map(|p| { let b = std::fs::read(&p).unwrap(); (p, b) }).collect();
    v.sort();
    v
}

fn ann_target(t: &FixturePlatform) -> PathBuf {
    t.root().join("Users/annexample")
}

#[test]
fn bundle_validation_and_default_mapping() {
    let c = capture(&[], None);
    let target = c.env.target();
    let svc = service(&target);
    let opened = svc.open_bundle(&c.bundle, true, |_| {}).unwrap();
    assert!(opened.validation.ok, "{:?}", opened.validation);
    assert!(opened.validation.manifest_hash_ok);
    let info = svc.target_info().unwrap();
    assert_eq!(info.computer_name, "ACCT-PC-21");
    let maps = default_mappings(&opened.manifest, &info.profiles);
    assert_eq!(maps.len(), 1);
    assert_eq!(maps[0].source_sid, ANN_SID);
    assert_eq!(maps[0].target_sid.as_deref(), Some(TARGET_SID), "mapped by display name");
    let _ = c.scan;
}

#[test]
fn dry_run_plan_writes_nothing_and_reports_conflicts() {
    let c = capture(&["browser-chrome", "printer:", "drive:", "wallpaper", "signatures"], None);
    let target = c.env.target();
    let before = snapshot(target.root());
    let svc = service(&target);
    let opened = svc.open_bundle(&c.bundle, true, |_| {}).unwrap();
    let plan = svc.plan(&opened, &request(&opened, &target)).unwrap();
    assert!(plan.dry_run);
    assert_eq!(snapshot(target.root()), before, "dry run must not modify the target");
    assert!(target.journal().is_empty(), "dry run must not apply system changes");

    let docs = plan.actions.iter().find(|a| a.item_id.ends_with(":documents")).unwrap();
    assert_eq!(docs.conflicts, 1, "Report.docx exists on the target");
    assert_eq!(docs.policy, CollisionPolicy::SkipExisting);
    assert!(docs.target_path.as_ref().unwrap().ends_with("Documents"));
    // Ordering: user files before browsers before drives before printers.
    let pos = |pred: &dyn Fn(&RestoreAction) -> bool| plan.actions.iter().position(|a| pred(a)).unwrap();
    assert!(pos(&|a| a.category == Category::UsersFiles) < pos(&|a| a.category == Category::Browsers));
    assert!(pos(&|a| a.category == Category::Browsers) < pos(&|a| a.category == Category::NetworkDrives));
    assert!(pos(&|a| a.category == Category::NetworkDrives) < pos(&|a| a.category == Category::Printers));
    // Exact system changes are shown before confirmation.
    assert!(plan.actions.iter().any(|a| a.system_changes.iter().any(|c| matches!(c, SystemChange::MapDrive { letter, .. } if letter == "H:"))));
    assert!(plan.actions.iter().any(|a| a.system_changes.iter().any(|c| matches!(c, SystemChange::RegistryValue { value_name, .. } if value_name == "WallPaper"))));
    let hp = plan.actions.iter().find(|a| a.display_name == "HP LaserJet 4th Floor").unwrap();
    assert!(hp.requires_admin && hp.blocked_reason.is_some(), "TCP/IP printer needs admin on a non-elevated target");
    let usb = plan.actions.iter().find(|a| a.display_name == "Brother HL-L2350DW");
    if let Some(usb) = usb {
        assert!(usb.system_changes.iter().all(|c| matches!(c, SystemChange::ManualChecklist { .. })));
    }
    // Bookmarks export precedes the browser profile restore.
    assert!(pos(&|a| a.id.starts_with("bookmarks:")) < pos(&|a| a.id.starts_with("restore:") && a.category == Category::Browsers));
}

#[test]
fn restore_with_default_policy_never_overwrites() {
    let c = capture(&["browser-chrome", "browser-firefox", "printer:", "drive:", "signatures"], None);
    let target = c.env.target();
    let svc = service(&target);
    let mut opened = svc.open_bundle(&c.bundle, true, |_| {}).unwrap();
    let req = request(&opened, &target);
    let s = svc.execute(&mut opened, &req).unwrap();
    assert_eq!(s.failures, 0, "{:?}", s.warnings);
    let ann = ann_target(&target);
    // Existing file untouched; new files restored byte-identical with timestamps.
    assert_eq!(std::fs::read_to_string(ann.join("Documents/Report.docx")).unwrap(), "Existing destination copy - must not be overwritten silently\n");
    let src_budget = c.env.source.root().join("Users/ann/Documents/Budget.xlsx");
    assert_eq!(std::fs::read(ann.join("Documents/Budget.xlsx")).unwrap(), std::fs::read(&src_budget).unwrap());
    assert_eq!(std::fs::metadata(ann.join("Documents/Budget.xlsx")).unwrap().modified().unwrap(), std::fs::metadata(&src_budget).unwrap().modified().unwrap());
    assert!(ann.join("Documents/Projects/Migration/plan.txt").is_file());
    assert!(ann.join("AppData/Roaming/Microsoft/Signatures/Work.htm").is_file());
    // Chrome Default already has Bookmarks on the target: skipped, but the HTML export exists.
    let chrome_bm = std::fs::read_to_string(ann.join("AppData/Local/Google/Chrome/User Data/Default/Bookmarks")).unwrap();
    assert!(chrome_bm.contains("New PC bookmark"));
    let exports = all_files(&ann.join("Desktop/Migrated Browser Data"));
    assert!(exports.iter().any(|p| p.to_string_lossy().contains("Google Chrome - Default bookmarks.html")));
    let ff = exports.iter().find(|p| p.to_string_lossy().contains("Mozilla Firefox")).expect("firefox export");
    let ff_html = std::fs::read_to_string(ff).unwrap();
    assert!(ff_html.contains("Contoso Intranet") && ff_html.contains("Employee Handbook &lt;HR&gt;"));
    // Firefox profile files go to Migrated Files.
    assert!(all_files(&ann.join("Migrated Files")).iter().any(|p| p.ends_with("places.sqlite")));
    // Drives mapped and shared printer connected through the (fixture) adapter.
    let j = target.journal();
    assert!(j.iter().any(|l| l.starts_with("map_drive H:")), "{j:?}");
    assert!(j.iter().any(|l| l.starts_with("connect_shared_printer")), "{j:?}");
    assert!(!j.iter().any(|l| l.starts_with("add_network_printer")), "admin-only action must be skipped when not elevated");
    // Report + history.
    assert!(Path::new(&s.report_html).is_file());
    let (m, ok) = migration_assistant_lib::bundle::read_manifest(&opened.layout).unwrap();
    assert!(ok, "manifest hash refreshed after appending history");
    assert_eq!(m.restore_history.len(), 1);
    assert_eq!(m.restore_history[0].target_computer, "ACCT-PC-21");
    // No secret ever reaches the target.
    for f in all_files(target.root()) {
        assert!(!contains(&std::fs::read(&f).unwrap(), SECRET_MARKER), "secret restored into {}", f.display());
    }
}

#[test]
fn rename_and_replace_policies() {
    let c = capture(&[], None);
    let target = c.env.target();
    let svc = service(&target);
    let ann = ann_target(&target);

    let mut opened = svc.open_bundle(&c.bundle, true, |_| {}).unwrap();
    let mut req = request(&opened, &target);
    req.policies.insert(Category::UsersFiles, CollisionPolicy::RenameIncoming);
    let s = svc.execute(&mut opened, &req).unwrap();
    assert!(s.files_renamed >= 1);
    assert!(ann.join("Documents/Report (migrated).docx").is_file());
    assert!(std::fs::read_to_string(ann.join("Documents/Report.docx")).unwrap().starts_with("Existing destination copy"));

    // Replace without the extra confirmation: refused per file, existing data intact.
    let mut opened = svc.open_bundle(&c.bundle, true, |_| {}).unwrap();
    let mut req = request(&opened, &target);
    req.policies.insert(Category::UsersFiles, CollisionPolicy::ReplaceAfterConfirmation);
    let s = svc.execute(&mut opened, &req).unwrap();
    assert!(s.failures > 0);
    assert!(std::fs::read_to_string(ann.join("Documents/Report.docx")).unwrap().starts_with("Existing destination copy"));

    // Replace with confirmation: existing file kept as .bak, new content written.
    let mut opened = svc.open_bundle(&c.bundle, true, |_| {}).unwrap();
    req.replace_confirmed = vec![Category::UsersFiles];
    let s = svc.execute(&mut opened, &req).unwrap();
    assert!(s.files_replaced >= 1, "{s:?}");
    assert!(std::fs::read_to_string(ann.join("Documents/Report.docx")).unwrap().starts_with("Quarterly report draft"));
    let baks: Vec<_> = all_files(&ann.join("Documents")).into_iter().filter(|p| p.to_string_lossy().contains("Report.docx.pre-migration-")).collect();
    assert_eq!(baks.len(), 1);
    assert!(std::fs::read_to_string(&baks[0]).unwrap().starts_with("Existing destination copy"));
}

#[test]
fn unconfirmed_categories_write_nothing() {
    let c = capture(&["drive:"], None);
    let target = c.env.target();
    let before = snapshot(target.root());
    let svc = service(&target);
    let mut opened = svc.open_bundle(&c.bundle, true, |_| {}).unwrap();
    let mut req = request(&opened, &target);
    req.confirmed_categories.clear();
    let s = svc.execute(&mut opened, &req).unwrap();
    assert_eq!(s.files_written, 0);
    assert!(target.journal().is_empty());
    let after: Vec<_> = snapshot(target.root());
    assert_eq!(after, before);
    assert!(s.task_results.iter().filter(|t| t.state != TaskState::Completed).all(|t| t.state == TaskState::Skipped));
}

#[test]
fn tampered_bundle_is_not_restorable() {
    let c = capture(&[], None);
    std::fs::write(c.bundle.join("users/ann/files/Documents/Budget.xlsx"), b"tampered").unwrap();
    let target = c.env.target();
    let svc = service(&target);
    let mut opened = svc.open_bundle(&c.bundle, true, |_| {}).unwrap();
    assert!(!opened.validation.ok);
    let req = request(&opened, &target);
    assert!(svc.execute(&mut opened, &req).is_err());
    assert!(!ann_target(&target).join("Documents/Budget.xlsx").exists());
}

#[test]
fn file_corrupted_after_validation_is_never_placed() {
    let c = capture(&[], None);
    let target = c.env.target();
    let svc = service(&target);
    let mut opened = svc.open_bundle(&c.bundle, true, |_| {}).unwrap();
    assert!(opened.validation.ok);
    // Corrupt after the full verification (quick re-check does not rehash).
    std::fs::write(c.bundle.join("users/ann/files/Documents/Budget.xlsx"), b"tampered").unwrap();
    let req = request(&opened, &target);
    let s = svc.execute(&mut opened, &req).unwrap();
    assert!(s.failures >= 1);
    assert!(!s.verified);
    assert!(s.warnings.iter().any(|w| w.code == WarningCode::HashMismatch));
    assert!(!ann_target(&target).join("Documents/Budget.xlsx").exists());
}

#[test]
fn encrypted_bundle_requires_correct_passphrase() {
    let c = capture(&["browser-chrome"], Some("correct horse battery staple"));
    let target = c.env.target();
    let svc = service(&target);
    let mut opened = svc.open_bundle(&c.bundle, true, |_| {}).unwrap();
    assert!(opened.validation.ok && opened.validation.encrypted);
    let mut req = request(&opened, &target);
    assert!(svc.execute(&mut opened, &req).is_err(), "passphrase required");
    req.passphrase = Some("wrong passphrase here".into());
    assert!(svc.execute(&mut opened, &req).is_err());
    req.passphrase = Some("correct horse battery staple".into());
    let s = svc.execute(&mut opened, &req).unwrap();
    assert_eq!(s.failures, 0, "{:?}", s.warnings);
    let ann = ann_target(&target);
    assert_eq!(
        std::fs::read(ann.join("Documents/Budget.xlsx")).unwrap(),
        std::fs::read(c.env.source.root().join("Users/ann/Documents/Budget.xlsx")).unwrap()
    );
    assert!(all_files(&ann.join("Desktop/Migrated Browser Data")).iter().any(|p| p.to_string_lossy().ends_with("bookmarks.html")));
}
