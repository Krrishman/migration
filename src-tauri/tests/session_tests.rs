//! Use-case tests for the Session layer behind the Tauri commands.

mod common;

use common::*;
use migration_assistant_lib::models::*;
use migration_assistant_lib::platform::fixture::FixturePlatform;
use migration_assistant_lib::progress::NullSink;
use migration_assistant_lib::session::Session;
use migration_assistant_lib::util::CancelToken;
use std::path::PathBuf;
use std::sync::Arc;

struct S {
    _env: Env,
    session: Session,
    exe_dir: PathBuf,
    usb: PathBuf,
}

fn session() -> S {
    let env = Env::new();
    let src = env.source.root().to_path_buf();
    let exe_dir = env.temp.path().join("usb-app");
    std::fs::create_dir_all(&exe_dir).unwrap();
    let platform = Arc::new(FixturePlatform::load(&src).unwrap());
    let session = Session::new(platform, &exe_dir.join("MigrationAssistant.exe"));
    let usb = env.dest_root.clone();
    S { _env: env, session, exe_dir, usb }
}

fn scan(s: &S) -> ScanResult {
    s.session.scan(&CancelToken::new(), &|_| {}).unwrap()
}

#[test]
fn authorization_is_required_and_persisted_portably() {
    let s = session();
    let st = s.session.status();
    assert!(!st.authorization_acknowledged);
    assert!(st.fixture_mode);
    assert_eq!(st.network_access, "none");
    assert!(st.paths.exe_dir_writable);
    assert!(matches!(s.session.scan(&CancelToken::new(), &|_| {}), Err(migration_assistant_lib::error::AppError::ConfirmationRequired(_))));
    s.session.acknowledge_authorization().unwrap();
    assert!(s.exe_dir.join("MigrationAssistantData/settings.json").is_file(), "stored beside the exe, not in AppData");
    // A new session from the same portable folder remembers it.
    let again = Session::new(s.session.platform.clone(), &s.exe_dir.join("MigrationAssistant.exe"));
    assert!(again.status().authorization_acknowledged);
    assert!(scan(&s).items.len() > 10);
}

#[test]
fn destination_validation() {
    let s = session();
    let root = s._env.source.root().to_path_buf();
    assert!(s.session.set_destination(&root.join("Windows").display().to_string()).is_err());
    assert!(s.session.set_destination(&root.join("does-not-exist").display().to_string()).is_err());
    let d = s.session.set_destination(&s.usb.display().to_string()).unwrap();
    assert_eq!(s.session.status().session_destination.as_deref(), Some(d.as_str()));
    let cfg = s.session.save_config().unwrap();
    assert!(cfg.ends_with("migration-assistant.config.json"));
}

#[test]
fn custom_folders_are_validated() {
    let s = session();
    s.session.acknowledge_authorization().unwrap();
    scan(&s);
    let root = s._env.source.root().to_path_buf();
    // System locations and their parents are refused.
    assert!(s.session.add_custom_folder(&root.join("Windows").display().to_string(), None).is_err());
    assert!(s.session.add_custom_folder(&root.join("Program Files/Contoso").display().to_string(), None).is_err());
    assert!(s.session.add_custom_folder(&root.display().to_string(), None).is_err(), "a folder containing Windows is refused");
    assert!(s.session.add_custom_folder(&root.join("ProgramData/Microsoft/Wlansvc").display().to_string(), None).is_err(), "Wi-Fi store refused");
    let projects = root.join("Users/ann/Documents/Projects");
    let item = s.session.add_custom_folder(&projects.display().to_string(), Some(ANN_SID)).unwrap();
    assert_eq!(item.restore_kind, RestoreKind::CustomFolder);
    assert_eq!(item.item_count, Some(1));
    assert!(s.session.add_custom_folder(&projects.display().to_string(), Some(ANN_SID)).is_err(), "no duplicates");
    assert!(s.session.current_scan().unwrap().items.iter().any(|i| i.id == item.id));
}

#[test]
fn generic_chromium_root_requires_confirmation_shape() {
    let s = session();
    s.session.acknowledge_authorization().unwrap();
    scan(&s);
    let root = s._env.source.root().to_path_buf();
    let not_browser = root.join("Users/ann/Documents");
    assert!(s.session.add_chromium_root(&not_browser.display().to_string(), "Fake", ANN_SID).is_err());
    let brave = root.join("Users/ann/AppData/Local/BraveSoftware/Brave-Browser/User Data");
    copy_tree(&root.join("Users/ann/AppData/Local/Google/Chrome/User Data"), &brave);
    let items = s.session.add_chromium_root(&brave.display().to_string(), "Brave", ANN_SID).unwrap();
    assert_eq!(items.len(), 2);
    assert!(items.iter().all(|i| i.opt_in_only && !i.selected_by_default));
    assert!(items.iter().all(|i| matches!(&i.restore_kind, RestoreKind::BrowserProfile { browser: BrowserKind::Chromium, .. })));
    // Outside the user's profile is refused.
    let outside = root.join("Brave User Data");
    copy_tree(&brave, &outside);
    assert!(s.session.add_chromium_root(&outside.display().to_string(), "Brave", ANN_SID).is_err());
}

#[test]
fn incomplete_bundle_deletion_requires_typed_id_and_refuses_verified() {
    let s = session();
    s.session.acknowledge_authorization().unwrap();
    let sc = scan(&s);
    let req = CaptureRequest {
        destination_root: s.usb.display().to_string(),
        selected_item_ids: default_ids(&sc),
        encryption: EncryptionRequest::default(),
        options: CaptureOptions::default(),
    };
    let done = s.session.capture(&req, Arc::new(NullSink)).unwrap();
    assert!(s.session.delete_incomplete_bundle(&done.bundle_path, &done.bundle_id).is_err(), "verified bundles are never deleted by the app");
    assert_eq!(s.session.list_bundles().len(), 1);

    // Simulate an interrupted capture by marking the manifest in progress.
    let layout = migration_assistant_lib::bundle::BundleLayout::open(std::path::Path::new(&done.bundle_path)).unwrap();
    let (mut m, _) = migration_assistant_lib::bundle::read_manifest(&layout).unwrap();
    m.status = BundleStatus::InProgress;
    migration_assistant_lib::bundle::write_manifest(&layout, &m).unwrap();
    assert!(s.session.delete_incomplete_bundle(&done.bundle_path, "wrong-id").is_err());
    assert!(layout.root.exists());
    s.session.delete_incomplete_bundle(&done.bundle_path, &done.bundle_id).unwrap();
    assert!(!layout.root.exists());
    assert!(s.usb.join("migrations").exists(), "only the bundle folder is removed");
}

#[test]
fn reports_and_restore_instructions() {
    let s = session();
    s.session.acknowledge_authorization().unwrap();
    let sc = scan(&s);
    let req = CaptureRequest {
        destination_root: s.usb.display().to_string(),
        selected_item_ids: default_ids(&sc),
        encryption: EncryptionRequest::default(),
        options: CaptureOptions::default(),
    };
    let done = s.session.capture(&req, Arc::new(NullSink)).unwrap();
    let txt = s.session.prepare_restore_instructions(&done.bundle_path).unwrap();
    let body = std::fs::read_to_string(&txt).unwrap();
    assert!(body.contains("Restore migration backup"));
    let view = s.session.load_report(&format!("{}/report.json", done.bundle_path)).unwrap();
    assert_eq!(view.kind, "json");
    assert_eq!(view.manifest.unwrap().bundle_id, done.bundle_id);
    assert!(view.html_path.is_some() && view.summary_html_path.is_some());
    assert!(s.session.load_report(&s.usb.display().to_string()).is_err());
    assert!(s.session.open_path(&format!("{}/manifest.sha256", done.bundle_path)).is_err(), "only folders and report files can be opened");
}
