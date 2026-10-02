//! Application session: the use-case layer behind the Tauri commands. It
//! owns the platform adapter, the current scan and restore state, and
//! enforces request-level validation. Keeping it free of Tauri types makes
//! every user flow testable headlessly.

use crate::app_paths::{self, PortableConfig, PortablePaths};
use crate::bundle::{self, BundleLayout};
use crate::capture::{preflight, CaptureEngine};
use crate::discovery::browsers::{discover_with_provider, ChromiumProvider};
use crate::discovery::{custom_folder_item, DiscoveryContext, DiscoveryService, ScanAccumulator};
use crate::error::{AppError, AppResult};
use crate::index::{BundleIndex, IndexedBundle};
use crate::models::*;
use crate::platform::SharedPlatform;
use crate::progress::ProgressSink;
use crate::restore::{default_mappings, OpenedBundle, RestoreService};
use crate::security::exclusions::ExclusionRules;
use crate::security::safe_path::{display_path, is_within};
use crate::util::CancelToken;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize)]
pub struct AppStatus {
    pub app_version: String,
    pub platform: String,
    pub fixture_mode: bool,
    pub elevated: bool,
    pub computer_name: String,
    pub paths: PortablePaths,
    pub authorization_acknowledged: bool,
    pub session_destination: Option<String>,
    pub network_access: &'static str,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Settings {
    authorization_acknowledged_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BundleOverview {
    pub validation: BundleValidation,
    pub manifest: Manifest,
    pub target: TargetInfo,
    pub suggested_mappings: Vec<UserMapping>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReportView {
    pub kind: String,
    pub manifest: Option<Manifest>,
    pub html_path: Option<String>,
    pub summary_html_path: Option<String>,
    pub bundle_path: Option<String>,
}

pub struct Session {
    pub platform: SharedPlatform,
    pub paths: PortablePaths,
    scan: Mutex<Option<ScanResult>>,
    session_destination: Mutex<Option<String>>,
    acknowledged: Mutex<bool>,
    capture_cancel: Mutex<Option<CancelToken>>,
    restore_cancel: Mutex<Option<CancelToken>>,
    opened: Mutex<Option<OpenedBundle>>,
}

impl Session {
    pub fn new(platform: SharedPlatform, exe_path: &Path) -> Self {
        let paths = app_paths::resolve(exe_path);
        let acknowledged = paths
            .data_dir
            .as_ref()
            .and_then(|d| std::fs::read(Path::new(d).join("settings.json")).ok())
            .and_then(|b| serde_json::from_slice::<Settings>(&b).ok())
            .is_some_and(|s| s.authorization_acknowledged_at.is_some());
        Self {
            platform,
            paths,
            scan: Mutex::new(None),
            session_destination: Mutex::new(None),
            acknowledged: Mutex::new(acknowledged),
            capture_cancel: Mutex::new(None),
            restore_cancel: Mutex::new(None),
            opened: Mutex::new(None),
        }
    }

    fn index(&self) -> Option<BundleIndex> {
        app_paths::ensure_data_dir(&self.paths).ok().flatten().and_then(|d| BundleIndex::open(&d).ok())
    }

    pub fn status(&self) -> AppStatus {
        AppStatus {
            app_version: crate::APP_VERSION.into(),
            platform: self.platform.name(),
            fixture_mode: self.platform.is_fixture(),
            elevated: self.platform.is_elevated(),
            computer_name: self.platform.machine_info().map(|m| m.computer_name).unwrap_or_default(),
            paths: self.paths.clone(),
            authorization_acknowledged: *self.acknowledged.lock(),
            session_destination: self.session_destination.lock().clone().or(self.paths.default_destination.clone()),
            network_access: "none",
        }
    }

    /// Record the authorization acknowledgement (persisted only in the portable data dir).
    pub fn acknowledge_authorization(&self) -> AppResult<()> {
        *self.acknowledged.lock() = true;
        if let Some(d) = app_paths::ensure_data_dir(&self.paths)? {
            crate::util::write_json_atomic(&d.join("settings.json"), &Settings { authorization_acknowledged_at: Some(chrono::Utc::now()) })?;
        }
        Ok(())
    }

    fn require_ack(&self) -> AppResult<()> {
        if *self.acknowledged.lock() {
            Ok(())
        } else {
            Err(AppError::ConfirmationRequired("Confirm that you are authorized to migrate data on this PC first.".into()))
        }
    }

    pub fn set_destination(&self, path: &str) -> AppResult<String> {
        let p = Path::new(path);
        if !preflight::is_writable_dir(p) {
            return Err(AppError::InvalidRequest(format!("{} is not a writable folder", p.display())));
        }
        let rules = ExclusionRules::new(self.platform.system_roots());
        if rules.system_roots.iter().any(|r| is_within(r, p)) {
            return Err(AppError::UnsafePath("Choose a destination outside Windows and Program Files.".into()));
        }
        *self.session_destination.lock() = Some(display_path(p));
        Ok(display_path(p))
    }

    pub fn save_config(&self) -> AppResult<String> {
        let dest = self.session_destination.lock().clone();
        app_paths::save_config(&self.paths, &PortableConfig { destination_root: dest })
    }

    pub fn disk_space(&self, path: &str) -> Option<DiskSpace> {
        self.platform.disk_space(Path::new(path))
    }

    // ---------------- scanning ----------------

    pub fn scan(&self, cancel: &CancelToken, progress: &(dyn Fn(ScanProgress) + Send + Sync)) -> AppResult<ScanResult> {
        self.require_ack()?;
        let r = DiscoveryService::new().scan(self.platform.as_ref(), cancel, true, progress)?;
        *self.scan.lock() = Some(r.clone());
        Ok(r)
    }

    pub fn current_scan(&self) -> Option<ScanResult> {
        self.scan.lock().clone()
    }

    fn ctx(&self) -> DiscoveryContext<'_> {
        DiscoveryContext {
            platform: self.platform.as_ref(),
            rules: ExclusionRules::new(self.platform.system_roots()),
            cancel: CancelToken::new(),
            processes: self.platform.running_processes(),
            measure_sizes: true,
        }
    }

    pub fn add_custom_folder(&self, path: &str, owner_sid: Option<&str>) -> AppResult<DiscoveryItem> {
        let mut guard = self.scan.lock();
        let scan = guard.as_mut().ok_or_else(|| AppError::InvalidRequest("Scan the PC first.".into()))?;
        let owner = owner_sid.and_then(|s| scan.users.iter().find(|u| u.sid == s)).cloned();
        let item = custom_folder_item(&self.ctx(), Path::new(path), owner.as_ref())?;
        if scan.items.iter().any(|i| i.id == item.id) {
            return Err(AppError::InvalidRequest("This folder is already in the list.".into()));
        }
        scan.items.push(item.clone());
        Ok(item)
    }

    /// Generic Chromium-family browser with a technician-confirmed "User Data" root.
    pub fn add_chromium_root(&self, path: &str, name: &str, owner_sid: &str) -> AppResult<Vec<DiscoveryItem>> {
        let mut guard = self.scan.lock();
        let scan = guard.as_mut().ok_or_else(|| AppError::InvalidRequest("Scan the PC first.".into()))?;
        let owner = scan.users.iter().find(|u| u.sid == owner_sid).cloned().ok_or_else(|| AppError::NotFound("user".into()))?;
        let root = crate::security::safe_path::canonicalize(Path::new(path))?;
        if !ChromiumProvider::looks_like_user_data(&root) {
            return Err(AppError::InvalidRequest("This folder does not look like a Chromium \"User Data\" folder (no Default\\Preferences).".into()));
        }
        if !is_within(&owner.profile_path, &root) {
            return Err(AppError::InvalidRequest("The browser folder must be inside the selected user's profile.".into()));
        }
        let provider = ChromiumProvider::generic(format!("{} (Chromium-family)", name.trim()), root);
        let mut acc = ScanAccumulator::default();
        discover_with_provider(&provider, &self.ctx(), &owner, &mut acc)?;
        let mut added = Vec::new();
        for mut i in acc.items {
            if scan.items.iter().any(|x| x.id == i.id) {
                continue;
            }
            i.opt_in_only = true;
            i.selected_by_default = false;
            added.push(i.clone());
            scan.items.push(i);
        }
        scan.browser_profiles.extend(acc.browser_profiles);
        Ok(added)
    }

    // ---------------- capture ----------------

    fn scan_or_err(&self) -> AppResult<ScanResult> {
        self.current_scan().ok_or_else(|| AppError::InvalidRequest("Scan the PC first.".into()))
    }

    pub fn preflight(&self, req: &CaptureRequest) -> AppResult<PreflightReport> {
        let scan = self.scan_or_err()?;
        Ok(preflight::run_preflight(self.platform.as_ref(), &scan, req))
    }

    pub fn capture(&self, req: &CaptureRequest, sink: Arc<dyn ProgressSink>) -> AppResult<CaptureSummary> {
        self.require_ack()?;
        let scan = self.scan_or_err()?;
        let cancel = CancelToken::new();
        *self.capture_cancel.lock() = Some(cancel.clone());
        let r = CaptureEngine { platform: self.platform.as_ref(), sink, cancel }.start(&scan, req);
        *self.capture_cancel.lock() = None;
        let s = r?;
        self.index_bundle(&s.bundle_path, "capture");
        Ok(s)
    }

    pub fn resume_capture(&self, bundle_path: &str, passphrase: Option<&str>, sink: Arc<dyn ProgressSink>) -> AppResult<CaptureSummary> {
        self.require_ack()?;
        let scan = self.scan_or_err()?;
        let cancel = CancelToken::new();
        *self.capture_cancel.lock() = Some(cancel.clone());
        let r = CaptureEngine { platform: self.platform.as_ref(), sink, cancel }.resume(&scan, Path::new(bundle_path), passphrase);
        *self.capture_cancel.lock() = None;
        let s = r?;
        self.index_bundle(&s.bundle_path, "resume");
        Ok(s)
    }

    pub fn cancel_capture(&self) -> bool {
        match self.capture_cancel.lock().as_ref() {
            Some(c) => {
                c.cancel();
                true
            }
            None => false,
        }
    }

    fn index_bundle(&self, path: &str, event: &str) {
        if let (Some(idx), Ok(layout)) = (self.index(), BundleLayout::open(Path::new(path))) {
            if let Ok((m, _)) = bundle::read_manifest(&layout) {
                let _ = idx.record(&m.bundle_id, path, &m.source_machine.computer_name, &m.created_at.to_rfc3339(), &format!("{:?}", m.status), event);
            }
        }
    }

    pub fn list_bundles(&self) -> Vec<IndexedBundle> {
        self.index().and_then(|i| i.list().ok()).unwrap_or_default()
    }

    /// Delete an incomplete bundle. Requires the bundle id as typed
    /// confirmation and refuses verified bundles or folders that are not
    /// Migration Assistant bundles.
    pub fn delete_incomplete_bundle(&self, path: &str, confirm_bundle_id: &str) -> AppResult<()> {
        let layout = BundleLayout::open(Path::new(path))?;
        let bytes = std::fs::read(layout.manifest_path()).map_err(|e| AppError::io(layout.manifest_path(), e))?;
        let v: serde_json::Value = serde_json::from_slice(&bytes)?;
        let id = v.get("bundle_id").and_then(|x| x.as_str()).unwrap_or_default();
        let status = v.get("status").and_then(|x| x.as_str()).unwrap_or_default();
        if id.is_empty() || id != confirm_bundle_id {
            return Err(AppError::ConfirmationRequired("Type the bundle ID exactly to confirm deletion.".into()));
        }
        if !matches!(status, "in_progress" | "canceled" | "completed_unverified") {
            return Err(AppError::InvalidRequest("Only incomplete or unverified bundles can be deleted from Migration Assistant.".into()));
        }
        if layout.root.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string()).as_deref() != Some(bundle::MIGRATIONS_DIR) {
            return Err(AppError::UnsafePath("The folder is not inside a \"migrations\" folder; delete it manually if intended.".into()));
        }
        for sub in bundle::SUBDIRS {
            if !layout.root.join(sub).is_dir() {
                return Err(AppError::UnsafePath("The folder does not have the bundle layout; delete it manually if intended.".into()));
            }
        }
        std::fs::remove_dir_all(&layout.root).map_err(|e| AppError::io(&layout.root, e))?;
        if let Some(idx) = self.index() {
            let _ = idx.forget(id);
        }
        Ok(())
    }

    /// Write plain-text restore instructions into the bundle.
    pub fn prepare_restore_instructions(&self, bundle_path: &str) -> AppResult<String> {
        let layout = BundleLayout::open(Path::new(bundle_path))?;
        let (m, _) = bundle::read_manifest(&layout)?;
        let mut text = format!(
            "Migration Assistant – restore instructions\n==========================================\n\nBundle: {}\nSource: {} ({} {})\nCaptured: {}\nStatus: {:?}\n\n",
            m.bundle_id,
            m.source_machine.computer_name,
            m.source_machine.os_name,
            m.source_machine.os_version,
            m.created_at.format("%Y-%m-%d %H:%M UTC"),
            m.status
        );
        for (i, step) in crate::reporting::restore_instructions(&m).iter().enumerate() {
            text.push_str(&format!("{}. {step}\n", i + 1));
        }
        text.push_str("\nPer-item notes:\n");
        for i in &m.items {
            for n in &i.restore_notes {
                text.push_str(&format!("- {}: {n}\n", i.display_name));
            }
        }
        let p = layout.root.join("RESTORE-INSTRUCTIONS.txt");
        crate::util::write_bytes_atomic(&p, text.as_bytes())?;
        Ok(display_path(&p))
    }

    // ---------------- restore ----------------

    fn restore_service(&self, sink: Arc<dyn ProgressSink>, cancel: CancelToken) -> RestoreService<'_> {
        RestoreService { platform: self.platform.as_ref(), sink, cancel }
    }

    pub fn open_bundle(&self, path: &str, on_file: impl FnMut(&str)) -> AppResult<BundleOverview> {
        self.require_ack()?;
        let svc = self.restore_service(Arc::new(crate::progress::NullSink), CancelToken::new());
        let opened = svc.open_bundle(Path::new(path), true, on_file)?;
        let target = svc.target_info()?;
        let overview = BundleOverview {
            suggested_mappings: default_mappings(&opened.manifest, &target.profiles),
            validation: opened.validation.clone(),
            manifest: opened.manifest.clone(),
            target,
        };
        *self.opened.lock() = Some(opened);
        Ok(overview)
    }

    pub fn target_info(&self) -> AppResult<TargetInfo> {
        self.restore_service(Arc::new(crate::progress::NullSink), CancelToken::new()).target_info()
    }

    pub fn plan_restore(&self, req: &RestoreRequest) -> AppResult<RestorePlan> {
        let guard = self.opened.lock();
        let opened = guard.as_ref().ok_or_else(|| AppError::InvalidRequest("Open a bundle first.".into()))?;
        self.restore_service(Arc::new(crate::progress::NullSink), CancelToken::new()).plan(opened, req)
    }

    pub fn execute_restore(&self, req: &RestoreRequest, sink: Arc<dyn ProgressSink>) -> AppResult<RestoreSummary> {
        self.require_ack()?;
        let cancel = CancelToken::new();
        *self.restore_cancel.lock() = Some(cancel.clone());
        let mut guard = self.opened.lock();
        let opened = guard.as_mut().ok_or_else(|| AppError::InvalidRequest("Open a bundle first.".into()))?;
        let r = self.restore_service(sink, cancel).execute(opened, req);
        *self.restore_cancel.lock() = None;
        let s = r?;
        let path = opened.layout.root.display().to_string();
        drop(guard);
        self.index_bundle(&path, "restore");
        Ok(s)
    }

    pub fn cancel_restore(&self) -> bool {
        match self.restore_cancel.lock().as_ref() {
            Some(c) => {
                c.cancel();
                true
            }
            None => false,
        }
    }

    // ---------------- reports & shell ----------------

    /// Open an existing report: a bundle folder, manifest.json, report.json or report.html.
    pub fn load_report(&self, path: &str) -> AppResult<ReportView> {
        let p = PathBuf::from(path);
        let root = if p.is_dir() { p.clone() } else { p.parent().map(Path::to_path_buf).unwrap_or_default() };
        let html = root.join("report.html");
        let summary = root.join("summary-report.html");
        let manifest = BundleLayout::open(&root).ok().and_then(|l| bundle::read_manifest(&l).ok()).map(|(m, _)| m);
        let kind = match p.extension().map(|e| e.to_string_lossy().to_lowercase()).as_deref() {
            Some("html") => "html",
            Some("json") => "json",
            _ => "bundle",
        };
        if manifest.is_none() && kind != "html" {
            return Err(AppError::NotFound("No Migration Assistant manifest was found next to this file.".into()));
        }
        Ok(ReportView {
            kind: kind.into(),
            manifest,
            html_path: if kind == "html" { Some(display_path(&p)) } else { html.is_file().then(|| display_path(&html)) },
            summary_html_path: summary.is_file().then(|| display_path(&summary)),
            bundle_path: root.join(bundle::MANIFEST_FILE).is_file().then(|| display_path(&root)),
        })
    }

    /// Open a folder or report with the system handler. Only existing local
    /// folders and .html/.json/.txt files are allowed.
    pub fn open_path(&self, path: &str) -> AppResult<()> {
        let p = Path::new(path);
        if p.is_dir() {
            return self.platform.open_folder(p);
        }
        let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        if p.is_file() && matches!(ext.as_str(), "html" | "json" | "txt") {
            // Reveal via the folder handler on the file itself (Explorer opens it with its default app).
            return crate::platform::fixture::open_file_with_system_handler(p);
        }
        Err(AppError::InvalidRequest("Only folders and report files can be opened.".into()))
    }

    pub fn restart_elevated(&self, reason_modules: &[String]) -> AppResult<()> {
        let mut args = vec!["--elevated-relaunch".to_string()];
        args.extend(reason_modules.iter().map(|m| format!("--for={m}")));
        self.platform.restart_elevated(&args)
    }
}
