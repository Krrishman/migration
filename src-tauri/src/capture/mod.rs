//! Capture engine: turns selected discovery items into a verified,
//! resumable migration bundle.
//!
//! Pipeline: preflight → bundle folder + initial manifest → one resumable
//! task per item (inventory items grouped per inventory file) → per-module
//! hash lists → structural verification → final manifest → reports.
//! Source data is only ever opened for reading.

pub mod checkpoint;
pub mod copier;
pub mod hashing;
pub mod preflight;

use crate::bundle::{self, BundleLayout};
use crate::error::{AppError, AppResult};
use crate::fs_walk::{walk, AllowList, WalkEvent, WalkOptions};
use crate::models::*;
use crate::platform::Platform;
use crate::progress::{Logger, ProgressSink, TaskTracker};
use crate::security::encryption::{self, BundleKey, ENCRYPTED_EXTENSION};
use crate::security::exclusions::ExclusionRules;
use crate::security::safe_path::{display_path, exceeds_max_path, relative_to, sanitize_component, sanitize_dir_name};
use crate::util::{now_utc, CancelToken};
use checkpoint::{Checkpoint, FileRecord, FileStatus};
use copier::{copy_verified, CopyError, CopySpec};
use hashing::HashEntry;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const FAT32_MAX_FILE: u64 = 4 * 1024 * 1024 * 1024 - 1;

pub struct CaptureEngine<'a> {
    pub platform: &'a dyn Platform,
    pub sink: Arc<dyn ProgressSink>,
    pub cancel: CancelToken,
}

/// Short stable key for file names derived from an item id.
pub fn task_key(id: &str) -> String {
    let prefix: String = id.split(':').next().unwrap_or("task").chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
    format!("{prefix}-{}", &hashing::sha256_bytes(id.as_bytes())[..16])
}

pub fn inventory_file(name: &str) -> String {
    match name {
        "printers" => "system/printers.json".into(),
        "network-drives" => "system/network-drives.json".into(),
        other => format!("inventory/{}.json", sanitize_dir_name(other)),
    }
}

fn root_name(item: &DiscoveryItem) -> String {
    match &item.payload {
        ItemPayload::Folder { root } | ItemPayload::AllowList { root, .. } | ItemPayload::FilesByExtension { root, .. } => {
            root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "root".into())
        }
        _ => "files".into(),
    }
}

/// Bundle-relative folder (or file, for inventory) that holds an item's payload.
pub fn item_bundle_path(item: &DiscoveryItem, user_dir: Option<&str>) -> String {
    let base = user_dir.map(|u| format!("users/{u}")).unwrap_or_else(|| "system".into());
    let short = &hashing::sha256_bytes(item.id.as_bytes())[..8];
    match &item.restore_kind {
        RestoreKind::KnownFolder { folder } => format!("{base}/files/{}", sanitize_component(folder.default_dir_name())),
        RestoreKind::CustomFolder => format!("{base}/files/Custom/{}-{short}", sanitize_component(&root_name(item))),
        RestoreKind::OneDriveLocal => format!("{base}/files/OneDrive/{}-{short}", sanitize_component(&root_name(item))),
        RestoreKind::PublicDesktop => "system/public-desktop".into(),
        RestoreKind::StartMenuShortcuts => format!("{base}/shortcuts/start-menu"),
        RestoreKind::TaskbarPins => format!("{base}/shortcuts/taskbar"),
        RestoreKind::QuickAccess => format!("{base}/shortcuts/quick-access"),
        RestoreKind::RecentItems => format!("{base}/shortcuts/recent"),
        RestoreKind::BrowserProfile { browser, profile_dir } => {
            format!("{base}/browsers/{}/{}-{short}", format!("{browser:?}").to_lowercase(), sanitize_component(profile_dir))
        }
        RestoreKind::OutlookSignatures => format!("{base}/outlook/signatures"),
        RestoreKind::OutlookTemplates => format!("{base}/outlook/templates"),
        RestoreKind::OutlookStationery => format!("{base}/outlook/stationery"),
        RestoreKind::PstFile => format!("{base}/outlook/pst/{short}"),
        RestoreKind::OfficeTemplates => format!("{base}/app-settings/office-{short}"),
        RestoreKind::Wallpaper => format!("{base}/personalization/wallpaper"),
        RestoreKind::Themes => format!("{base}/personalization/themes"),
        RestoreKind::StickyNotes => format!("{base}/personalization/sticky-notes"),
        RestoreKind::MappedDrives => inventory_file("network-drives"),
        RestoreKind::Printers => inventory_file("printers"),
        RestoreKind::Inventory { name } => inventory_file(name),
    }
}

fn payload_root(item: &DiscoveryItem) -> Option<PathBuf> {
    match &item.payload {
        ItemPayload::Folder { root } | ItemPayload::AllowList { root, .. } | ItemPayload::FilesByExtension { root, .. } => Some(root.clone()),
        ItemPayload::Files { files } => files.first().and_then(|f| f.parent().map(Path::to_path_buf)),
        _ => None,
    }
}

/// Assign human-readable, unique per-user folder names.
fn user_dirs(users: &[&UserProfile]) -> HashMap<String, String> {
    let mut sorted: Vec<&&UserProfile> = users.iter().collect();
    sorted.sort_by(|a, b| a.sid.cmp(&b.sid));
    let mut used: HashSet<String> = HashSet::new();
    let mut out = HashMap::new();
    for u in sorted {
        let short = u.account_name.rsplit('\\').next().unwrap_or(&u.account_name);
        let base = sanitize_dir_name(short);
        let mut name = base.clone();
        let mut n = 2;
        while !used.insert(name.to_lowercase()) {
            name = format!("{base}-{n}");
            n += 1;
        }
        out.insert(u.sid.clone(), name);
    }
    out
}

/// One unit of independently tracked work.
enum TaskKind {
    Files(DiscoveryItem),
    Inventory { name: String, items: Vec<DiscoveryItem> },
}

struct PlannedTask {
    id: String,
    category: Category,
    display_name: String,
    kind: TaskKind,
}

fn plan_tasks(items: &[DiscoveryItem]) -> Vec<PlannedTask> {
    let mut tasks = Vec::new();
    let mut inventories: BTreeMap<String, Vec<DiscoveryItem>> = BTreeMap::new();
    for i in items {
        match &i.payload {
            ItemPayload::Inventory { name } => inventories.entry(name.clone()).or_default().push(i.clone()),
            ItemPayload::None => {}
            _ => tasks.push(PlannedTask {
                id: i.id.clone(),
                category: i.category,
                display_name: match &i.owner {
                    Some(o) => format!("{} ({})", i.display_name, o.account_name),
                    None => i.display_name.clone(),
                },
                kind: TaskKind::Files(i.clone()),
            }),
        }
    }
    for (name, items) in inventories {
        let category = items[0].category;
        let display_name = match name.as_str() {
            "machine" => "Computer inventory".to_string(),
            "profiles" => "User profile list".to_string(),
            "applications" => "Installed applications inventory".to_string(),
            "printers" => format!("Printer inventory ({} selected)", items.len()),
            "network-drives" => format!("Mapped drive inventory ({} selected)", items.len()),
            "outlook" => "Outlook profile inventory".to_string(),
            other => format!("Inventory: {other}"),
        };
        tasks.push(PlannedTask { id: format!("inventory:{name}"), category, display_name, kind: TaskKind::Inventory { name, items } });
    }
    // Stable order: by restore/category order, then name, so progress UI is predictable.
    tasks.sort_by(|a, b| a.category.restore_order().cmp(&b.category.restore_order()).then(a.display_name.cmp(&b.display_name)));
    tasks
}

struct Run<'a> {
    platform: &'a dyn Platform,
    layout: BundleLayout,
    manifest: Manifest,
    scan: &'a ScanResult,
    options: CaptureOptions,
    key: Option<BundleKey>,
    logger: Logger,
    checkpoint: Checkpoint,
    rules: ExclusionRules,
    fat32: bool,
    cancel: CancelToken,
    results: Vec<TaskProgress>,
    resumed: bool,
}

impl<'a> CaptureEngine<'a> {
    pub fn preflight(&self, scan: &ScanResult, req: &CaptureRequest) -> PreflightReport {
        preflight::run_preflight(self.platform, scan, req)
    }

    /// Start a new capture. Fails before writing anything if preflight blocks.
    pub fn start(&self, scan: &ScanResult, req: &CaptureRequest) -> AppResult<CaptureSummary> {
        let pf = self.preflight(scan, req);
        if !pf.blocking_errors.is_empty() {
            return Err(AppError::InvalidRequest(pf.blocking_errors.join(" ")));
        }
        let items: Vec<DiscoveryItem> = req.selected_item_ids.iter().filter_map(|id| scan.items.iter().find(|i| &i.id == id).cloned()).collect();
        let (encryption_meta, key) = if req.encryption.enabled {
            let pass = req.encryption.passphrase.as_deref().ok_or_else(|| AppError::Crypto("passphrase required".into()))?;
            let (m, k) = encryption::setup_bundle_encryption(pass)?;
            (m, Some(k))
        } else {
            (EncryptionMetadata::disabled(), None)
        };
        let bundle_id = uuid::Uuid::new_v4().to_string();
        let layout = BundleLayout::create(Path::new(&req.destination_root), &scan.machine.computer_name, &bundle_id)?;
        bundle::write_request(&layout, req)?;
        let manifest = self.initial_manifest(scan, req, &items, &bundle_id, encryption_meta, &pf);
        bundle::write_manifest(&layout, &manifest)?;
        self.run(layout, manifest, scan, items, req.options.clone(), key, false)
    }

    /// Resume an interrupted or canceled capture from its checkpoints.
    pub fn resume(&self, scan: &ScanResult, bundle_path: &Path, passphrase: Option<&str>) -> AppResult<CaptureSummary> {
        let layout = BundleLayout::open(bundle_path)?;
        let (manifest, _) = bundle::read_manifest(&layout)?;
        if !matches!(manifest.status, BundleStatus::InProgress | BundleStatus::Canceled) {
            return Err(AppError::InvalidRequest("This bundle is already complete; start a new capture instead.".into()));
        }
        if !manifest.source_machine.computer_name.eq_ignore_ascii_case(&scan.machine.computer_name) {
            return Err(AppError::InvalidRequest(format!(
                "This bundle was started on {}; resume it on that computer.",
                manifest.source_machine.computer_name
            )));
        }
        let req = bundle::read_request(&layout)?;
        let key = if manifest.encryption.enabled {
            let p = passphrase.ok_or_else(|| AppError::Crypto("this bundle is encrypted; enter its passphrase to resume".into()))?;
            Some(encryption::unlock_bundle(&manifest.encryption, p)?)
        } else {
            None
        };
        let mut items = Vec::new();
        for id in &req.selected_item_ids {
            match scan.items.iter().find(|i| &i.id == id) {
                Some(i) => items.push(i.clone()),
                None => return Err(AppError::InvalidRequest(format!("Selected item {id} was not found by the current scan. Rescan, then resume."))),
            }
        }
        self.run(layout, manifest, scan, items, req.options, key, true)
    }

    fn initial_manifest(
        &self,
        scan: &ScanResult,
        req: &CaptureRequest,
        items: &[DiscoveryItem],
        bundle_id: &str,
        encryption: EncryptionMetadata,
        pf: &PreflightReport,
    ) -> Manifest {
        let owners: Vec<&UserProfile> = scan.users.iter().filter(|u| items.iter().any(|i| i.owner.as_ref().is_some_and(|o| o.sid == u.sid))).collect();
        let dirs = user_dirs(&owners);
        let users = owners
            .iter()
            .map(|u| ManifestUser {
                sid: u.sid.clone(),
                account_name: u.account_name.clone(),
                display_name: u.display_name.clone(),
                profile_path: display_path(&u.profile_path),
                last_use: u.last_use,
                profile_size: u.size_bytes,
                bundle_dir: format!("users/{}", dirs[&u.sid]),
                selected_modules: items.iter().filter(|i| i.owner.as_ref().is_some_and(|o| o.sid == u.sid)).map(|i| i.id.clone()).collect(),
            })
            .collect();
        let mitems = items
            .iter()
            .map(|i| {
                let owner_profile = i.owner.as_ref().and_then(|o| scan.users.iter().find(|u| u.sid == o.sid));
                let profile_relative = match (owner_profile, payload_root(i)) {
                    (Some(u), Some(root)) => relative_to(&u.profile_path, &root),
                    _ => None,
                };
                ManifestItem {
                    id: i.id.clone(),
                    category: i.category,
                    display_name: i.display_name.clone(),
                    source_path: match &i.source {
                        SourceRef::Path { path } => display_path(path),
                        SourceRef::Config { reference } => reference.clone(),
                    },
                    owner: i.owner.clone(),
                    profile_relative,
                    bundle_path: item_bundle_path(i, i.owner.as_ref().map(|o| dirs[&o.sid].as_str())),
                    restore_kind: i.restore_kind.clone(),
                    support: i.support,
                    estimated_size: i.estimated_size,
                    captured_bytes: 0,
                    captured_files: 0,
                    skipped_files: 0,
                    hash_status: HashStatus::NotHashed,
                    hash_list: None,
                    hash_list_sha256: None,
                    capture_status: CaptureStatus::Pending,
                    warnings: i.warnings.clone(),
                    restore_notes: i.restore_notes.clone(),
                }
            })
            .collect();
        let selected_inv = |name: &str| items.iter().any(|i| matches!(&i.payload, ItemPayload::Inventory { name: n } if n == name));
        let selected_ids: HashSet<&str> = items.iter().map(|i| i.id.as_str()).collect();
        let mut modules: Vec<String> = items.iter().map(|i| format!("{:?}", i.category)).collect();
        modules.sort();
        modules.dedup();
        Manifest {
            schema_version: SCHEMA_VERSION.into(),
            bundle_id: bundle_id.into(),
            app_version: crate::APP_VERSION.into(),
            created_at: now_utc(),
            completed_at: None,
            status: BundleStatus::InProgress,
            source_machine: SourceMachine {
                computer_name: scan.machine.computer_name.clone(),
                os_name: scan.machine.os_name.clone(),
                os_version: scan.machine.os_version.clone(),
                os_build: scan.machine.os_build.clone(),
                architecture: scan.machine.architecture.clone(),
                time_zone: scan.machine.time_zone.clone(),
                join_state: scan.machine.join_state,
                join_name: scan.machine.join_name.clone(),
                device_id: None,
            },
            destination_root: req.destination_root.clone(),
            elevated: scan.elevated,
            encryption,
            users,
            selected_modules: modules,
            items: mitems,
            printers: scan
                .printers
                .iter()
                .filter(|p| selected_ids.contains(crate::discovery::item_id("printer", None, &p.name).as_str()))
                .cloned()
                .collect(),
            mapped_drives: scan
                .mapped_drives
                .iter()
                .filter(|d| selected_ids.contains(crate::discovery::item_id("drive", None, &d.letter).as_str()))
                .cloned()
                .collect(),
            applications: if selected_inv("applications") { scan.applications.clone() } else { vec![] },
            browser_profiles: scan
                .browser_profiles
                .iter()
                .filter(|b| items.iter().any(|i| matches!(&i.restore_kind, RestoreKind::BrowserProfile { browser, profile_dir } if *browser == b.browser && *profile_dir == b.profile_dir) && i.owner.as_ref().is_some_and(|o| o.sid == b.owner.sid)))
                .cloned()
                .collect(),
            restore_compatibility_notes: vec![
                format!("Captured from {} {} (build {}).", scan.machine.os_name, scan.machine.os_version, scan.machine.os_build),
                "Files restore to any Windows 10/11 PC. Start menu, taskbar and Quick Access layouts are best effort.".into(),
                "Passwords, cookies, credentials, Wi-Fi keys and application licenses are never part of a bundle.".into(),
                "Printers need compatible drivers on the destination; drivers are never installed automatically.".into(),
            ],
            log_summary: LogSummary { log_file: "logs/capture.log.jsonl".into(), ..Default::default() },
            capacity: CapacitySnapshot {
                source_free_bytes: pf.source_free_bytes,
                source_total_bytes: None,
                destination_free_bytes: pf.destination_free_bytes,
                destination_total_bytes: self.platform.disk_space(Path::new(&req.destination_root)).map(|d| d.total_bytes),
                estimated_bundle_bytes: pf.estimated_bytes,
            },
            integrity: IntegrityMetadata::default(),
            exclusions: ExclusionRules::new(self.platform.system_roots()).with_cache(req.options.include_browser_cache).manifest_exclusions(),
            restore_history: vec![],
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn run(
        &self,
        layout: BundleLayout,
        mut manifest: Manifest,
        scan: &ScanResult,
        items: Vec<DiscoveryItem>,
        options: CaptureOptions,
        key: Option<BundleKey>,
        resumed: bool,
    ) -> AppResult<CaptureSummary> {
        let logger = Logger::new(&layout.logs().join("capture.log.jsonl"), self.sink.clone());
        let checkpoint = Checkpoint::open(&layout.logs().join("checkpoint.sqlite"))?;
        let fat32 = self
            .platform
            .file_system_of(&layout.root)
            .is_some_and(|f| f.eq_ignore_ascii_case("fat32") || f.eq_ignore_ascii_case("vfat") || f.eq_ignore_ascii_case("fat"));
        logger.info(
            None,
            format!(
                "{} capture of {} item(s) from {} into {} (encryption: {}, platform: {})",
                if resumed { "Resuming" } else { "Starting" },
                items.len(),
                scan.machine.computer_name,
                layout.root.display(),
                if key.is_some() { "on" } else { "off" },
                self.platform.name()
            ),
        );
        if resumed {
            manifest.status = BundleStatus::InProgress;
        }
        let mut run = Run {
            platform: self.platform,
            rules: ExclusionRules::new(self.platform.system_roots()).with_cache(options.include_browser_cache),
            layout,
            manifest,
            scan,
            options,
            key,
            logger,
            checkpoint,
            fat32,
            cancel: self.cancel.clone(),
            results: vec![],
            resumed,
        };
        let tasks = plan_tasks(&items);
        // Announce every task as queued so the UI can render all cards immediately.
        let mut trackers: Vec<TaskTracker> = tasks.iter().map(|t| TaskTracker::new(&t.id, t.category, &t.display_name, self.sink.clone())).collect();
        for t in &mut trackers {
            t.emit(true);
        }
        let mut canceled = false;
        for (task, mut tracker) in tasks.into_iter().zip(trackers) {
            if canceled || run.cancel.is_canceled() {
                canceled = true;
                tracker.set_state(TaskState::Canceled);
                run.results.push(tracker.progress.clone());
                continue;
            }
            tracker.start();
            let outcome = match &task.kind {
                TaskKind::Files(item) => run.file_task(item, &mut tracker),
                TaskKind::Inventory { name, items } => run.inventory_task(&task.id, name, items, &mut tracker),
            };
            match outcome {
                Ok(()) => {}
                Err(AppError::Canceled) => {
                    canceled = true;
                    tracker.set_state(TaskState::Canceled);
                    run.logger.warn(Some(&task.id), "Task canceled by the technician; completed files are kept and the capture can be resumed.");
                }
                Err(e) => {
                    tracker.progress.error = Some(e.to_string());
                    tracker.set_state(TaskState::Failed);
                    run.logger.error(Some(&task.id), format!("{}: {e}", task.display_name));
                    run.mark_items(&task, CaptureStatus::Failed);
                }
            }
            run.results.push(tracker.progress.clone());
            bundle::write_manifest(&run.layout, &run.manifest)?;
        }
        run.finish(canceled)
    }
}

impl Run<'_> {
    fn mark_items(&mut self, task: &PlannedTask, status: CaptureStatus) {
        let ids: Vec<String> = match &task.kind {
            TaskKind::Files(i) => vec![i.id.clone()],
            TaskKind::Inventory { items, .. } => items.iter().map(|i| i.id.clone()).collect(),
        };
        for m in self.manifest.items.iter_mut().filter(|m| ids.contains(&m.id)) {
            m.capture_status = status;
        }
    }

    fn manifest_item(&mut self, id: &str) -> AppResult<&mut ManifestItem> {
        self.manifest.items.iter_mut().find(|m| m.id == id).ok_or_else(|| AppError::NotFound(format!("manifest item {id}")))
    }

    /// Enumerate an item's source files: (absolute path, item-relative path, size, mtime).
    fn enumerate(&self, item: &DiscoveryItem, tracker: &mut TaskTracker, warnings: &mut Vec<Warning>) -> AppResult<Vec<(PathBuf, String, u64, i64)>> {
        let mut files = Vec::new();
        let mut cloud = 0u64;
        let mut denied = 0u64;
        let mut links = 0u64;
        let mut sensitive = 0u64;
        let mut long = 0u64;
        let mut collect = |ev: WalkEvent<'_>, filter: Option<&[String]>| match ev {
            WalkEvent::File { path, rel, size, modified, flags } => {
                if let Some(exts) = filter {
                    if !crate::discovery::has_extension(path, exts) {
                        return;
                    }
                }
                if flags.is_cloud_placeholder || flags.is_offline {
                    cloud += 1;
                    return;
                }
                if exceeds_max_path(path) {
                    long += 1;
                }
                let mtime = modified.and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(0);
                files.push((path.to_path_buf(), rel.to_string(), size, mtime));
            }
            WalkEvent::Excluded { reason, .. } => {
                if reason.is_sensitive() {
                    sensitive += 1;
                }
            }
            WalkEvent::LinkSkipped { .. } => links += 1,
            WalkEvent::Error { error, .. } => {
                if error.kind() == std::io::ErrorKind::PermissionDenied {
                    denied += 1;
                }
            }
        };
        match &item.payload {
            ItemPayload::Folder { root } => walk(root, &self.rules, None, WalkOptions::default(), &self.cancel, |e| collect(e, None))?,
            ItemPayload::AllowList { root, files: f, dirs } => {
                let allow = AllowList { files: f.clone(), dirs: dirs.clone() };
                walk(root, &self.rules, Some(&allow), WalkOptions::default(), &self.cancel, |e| collect(e, None))?
            }
            ItemPayload::FilesByExtension { root, extensions } => {
                walk(root, &self.rules, None, WalkOptions { non_recursive: true }, &self.cancel, |e| collect(e, Some(extensions)))?
            }
            ItemPayload::Files { files: list } => {
                let mut used = HashSet::new();
                for f in list {
                    if let Some(reason) = self.rules.classify(f, false) {
                        warnings.push(Warning::warn(WarningCode::ExcludedPath, reason.describe()).with_path(display_path(f)));
                        continue;
                    }
                    match std::fs::symlink_metadata(f) {
                        Ok(m) if m.file_type().is_symlink() => links += 1,
                        Ok(m) => {
                            let name = f.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "file".into());
                            let mut rel = name.clone();
                            let mut n = 2;
                            while !used.insert(rel.to_lowercase()) {
                                rel = format!("{n}-{name}");
                                n += 1;
                            }
                            let mtime = m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(0);
                            files.push((f.clone(), rel, m.len(), mtime));
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => denied += 1,
                        Err(_) => warnings.push(Warning::warn(WarningCode::Skipped, "File no longer exists").with_path(display_path(f))),
                    }
                }
            }
            ItemPayload::Inventory { .. } | ItemPayload::None => {}
        }
        let mut add = |w: Warning, tracker: &mut TaskTracker| {
            tracker.warn();
            warnings.push(w);
        };
        if cloud > 0 {
            add(Warning::warn(WarningCode::CloudPlaceholder, format!("{cloud} online-only cloud file(s) skipped (not downloaded).")), tracker);
        }
        if denied > 0 {
            add(Warning::warn(WarningCode::AccessDenied, format!("{denied} folder(s)/file(s) skipped: access denied.")), tracker);
        }
        if links > 0 {
            self.logger.info(Some(&item.id), format!("{links} link(s)/junction(s) not followed."));
        }
        if sensitive > 0 {
            self.logger.info(Some(&item.id), format!("{sensitive} protected item(s) excluded (credentials, keys, cookies or tokens)."));
        }
        if long > 0 {
            add(Warning::info(WarningCode::LongPath, format!("{long} file(s) have paths over 260 characters.")), tracker);
        }
        Ok(files)
    }

    fn file_task(&mut self, item: &DiscoveryItem, tracker: &mut TaskTracker) -> AppResult<()> {
        let task_id = item.id.clone();
        if self.resumed && self.checkpoint.task_state(&task_id)?.as_deref() == Some("completed") {
            if let Some(m) = self.manifest.items.iter().find(|m| m.id == task_id) {
                if matches!(m.capture_status, CaptureStatus::Captured | CaptureStatus::CapturedWithWarnings) {
                    tracker.progress.bytes_done = m.captured_bytes;
                    tracker.progress.items_done = m.captured_files;
                    tracker.set_totals(Some(m.captured_bytes), Some(m.captured_files));
                    self.logger.info(Some(&task_id), "Already completed in a previous session; skipped.");
                    tracker.set_state(if m.capture_status == CaptureStatus::Captured { TaskState::Completed } else { TaskState::CompletedWithWarnings });
                    return Ok(());
                }
            }
        }
        tracker.set_state(TaskState::Scanning);
        self.checkpoint.set_task_state(&task_id, "running")?;
        let mut warnings: Vec<Warning> = Vec::new();
        let files = self.enumerate(item, tracker, &mut warnings)?;
        let total_bytes: u64 = files.iter().map(|f| f.2).sum();
        tracker.set_totals(Some(total_bytes), Some(files.len() as u64));
        let rels: HashSet<String> = files.iter().map(|f| f.1.clone()).collect();
        self.checkpoint.retain_only(&task_id, &rels)?;

        let bundle_path = self.manifest_item(&task_id)?.bundle_path.clone();
        tracker.set_state(TaskState::Copying);
        let mut entries: Vec<HashEntry> = Vec::new();
        let mut plain_entries: Vec<HashEntry> = Vec::new();
        let (mut captured_bytes, mut captured_files, mut skipped, mut failed, mut mismatches) = (0u64, 0u64, 0u64, 0u64, 0u64);
        let suffix = if self.key.is_some() { format!(".{ENCRYPTED_EXTENSION}") } else { String::new() };

        for (src, rel, size, mtime) in &files {
            self.cancel.check()?;
            let stored_rel = format!("{bundle_path}/{rel}{suffix}");
            let dest = self.layout.resolve(&stored_rel)?;
            tracker.progress.current_path = Some(display_path(src));

            // Resume: reuse a file finished in a previous session when the source is unchanged.
            if let Some(rec) = self.checkpoint.get(&task_id, rel)? {
                if rec.status == FileStatus::Done && rec.size == *size && rec.mtime == *mtime {
                    if let (Some(h), Some(ph), Ok(meta)) = (&rec.stored_hash, &rec.plain_hash, std::fs::metadata(&dest)) {
                        let intact = Some(meta.len()) == rec.stored_size && (!self.options.verify_after_copy || hashing::sha256_file(&dest).ok().as_ref() == Some(h));
                        if intact {
                            entries.push(HashEntry { sha256: h.clone(), path: stored_rel.clone() });
                            plain_entries.push(HashEntry { sha256: ph.clone(), path: stored_rel.clone() });
                            captured_bytes += size;
                            captured_files += 1;
                            tracker.advance(*size, 1, None);
                            continue;
                        }
                    }
                }
            }

            if self.fat32 && *size > FAT32_MAX_FILE {
                skipped += 1;
                tracker.warn();
                warnings.push(Warning::warn(WarningCode::LargeItem, "File is 4 GB or larger and cannot be stored on a FAT32 destination.").with_path(display_path(src)));
                self.logger.warn(Some(&task_id), format!("Skipped (FAT32 4 GB limit): {}", display_path(src)));
                tracker.advance(*size, 1, None);
                continue;
            }

            let mut rec = FileRecord {
                task_id: task_id.clone(),
                rel: rel.clone(),
                size: *size,
                mtime: *mtime,
                status: FileStatus::Pending,
                stored_path: Some(stored_rel.clone()),
                stored_hash: None,
                plain_hash: None,
                stored_size: None,
            };
            self.checkpoint.upsert(&rec, None)?;
            let spec = CopySpec {
                src,
                dest: &dest,
                key: self.key.as_ref(),
                verify: self.options.verify_after_copy,
                max_retries: self.options.max_retries,
                cancel: &self.cancel,
                platform: self.platform,
            };
            let mut reported = 0u64;
            let result = {
                let tracker_ref = &mut *tracker;
                copy_verified(&spec, &mut |n| {
                    // Never report more than the file's size (retries restart from zero).
                    let n = n.min(size.saturating_sub(reported));
                    reported += n;
                    tracker_ref.advance(n, 0, None);
                })
            };
            match result {
                Ok(o) => {
                    for _ in 0..o.retries {
                        tracker.retried();
                    }
                    if o.retries > 0 {
                        self.logger.info(Some(&task_id), format!("Copied after {} retr{}: {}", o.retries, if o.retries == 1 { "y" } else { "ies" }, display_path(src)));
                    }
                    if o.plain_bytes != *size {
                        tracker.warn();
                        warnings.push(Warning::warn(WarningCode::Other, "File changed while it was being copied; the copied version is the one read.").with_path(display_path(src)));
                    }
                    rec.status = FileStatus::Done;
                    rec.stored_hash = Some(o.stored_hash.clone());
                    rec.plain_hash = Some(o.plain_hash.clone());
                    rec.stored_size = Some(o.stored_bytes);
                    self.checkpoint.upsert(&rec, None)?;
                    entries.push(HashEntry { sha256: o.stored_hash, path: stored_rel.clone() });
                    plain_entries.push(HashEntry { sha256: o.plain_hash, path: stored_rel });
                    captured_bytes += o.plain_bytes;
                    captured_files += 1;
                    tracker.advance(size.saturating_sub(reported), 1, None);
                }
                Err(CopyError::Canceled) => return Err(AppError::Canceled),
                Err(e) => {
                    let (status, code) = match &e {
                        CopyError::Locked(_) => (FileStatus::Skipped, WarningCode::LockedFile),
                        CopyError::AccessDenied(_) => (FileStatus::Skipped, WarningCode::AccessDenied),
                        CopyError::HashMismatch => (FileStatus::Failed, WarningCode::HashMismatch),
                        _ => (FileStatus::Failed, WarningCode::Other),
                    };
                    if matches!(e, CopyError::Locked(_)) {
                        for _ in 0..self.options.max_retries {
                            tracker.retried();
                        }
                    }
                    let skippable = status == FileStatus::Skipped && (self.options.skip_locked_files || code == WarningCode::AccessDenied);
                    rec.status = if skippable { FileStatus::Skipped } else { FileStatus::Failed };
                    self.checkpoint.upsert(&rec, Some(&e.to_string()))?;
                    tracker.warn();
                    warnings.push(Warning::new(code, if skippable { Severity::Warning } else { Severity::Error }, e.to_string()).with_path(display_path(src)));
                    if skippable {
                        skipped += 1;
                        self.logger.warn(Some(&task_id), format!("Skipped {}: {e}", display_path(src)));
                    } else {
                        failed += 1;
                        if code == WarningCode::HashMismatch {
                            mismatches += 1;
                        }
                        self.logger.error(Some(&task_id), format!("Failed {}: {e}", display_path(src)));
                    }
                    tracker.advance(size.saturating_sub(reported), 1, None);
                }
            }
        }

        tracker.set_state(TaskState::Verifying);
        let key = task_key(&task_id);
        let list_rel = format!("hashes/{key}.sha256");
        let digest = hashing::write_hash_list(&self.layout.resolve(&list_rel)?, &entries)?;
        if self.key.is_some() {
            hashing::write_hash_list(&self.layout.resolve(&format!("hashes/{key}.plain.sha256"))?, &plain_entries)?;
        }
        let verify = self.options.verify_after_copy;
        let m = self.manifest_item(&task_id)?;
        m.captured_bytes = captured_bytes;
        m.captured_files = captured_files;
        m.skipped_files = skipped + failed;
        m.hash_list = Some(list_rel);
        m.hash_list_sha256 = Some(digest);
        m.hash_status = if mismatches > 0 {
            HashStatus::Mismatch
        } else if verify {
            HashStatus::Verified
        } else {
            HashStatus::Hashed
        };
        m.warnings.extend(warnings.iter().cloned());
        let state = if failed > 0 && captured_files == 0 && !files.is_empty() {
            m.capture_status = CaptureStatus::Failed;
            TaskState::Failed
        } else if failed > 0 || skipped > 0 || warnings.iter().any(|w| w.severity >= Severity::Warning) {
            m.capture_status = CaptureStatus::CapturedWithWarnings;
            TaskState::CompletedWithWarnings
        } else {
            m.capture_status = CaptureStatus::Captured;
            TaskState::Completed
        };
        if failed > 0 {
            tracker.progress.error = Some(format!("{failed} file(s) failed"));
        }
        self.checkpoint.set_task_state(&task_id, "completed")?;
        tracker.progress.current_path = None;
        tracker.set_state(state);
        self.logger.info(Some(&task_id), format!("{}: {captured_files} file(s), {captured_bytes} bytes, {skipped} skipped, {failed} failed", item.display_name));
        Ok(())
    }

    fn inventory_task(&mut self, task_id: &str, name: &str, items: &[DiscoveryItem], tracker: &mut TaskTracker) -> AppResult<()> {
        tracker.set_state(TaskState::Copying);
        let rel = inventory_file(name);
        let path = self.layout.resolve(&rel)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
        }
        let ids: HashSet<&str> = items.iter().map(|i| i.id.as_str()).collect();
        let scan = self.scan;
        let value = match name {
            "machine" => serde_json::to_value(&scan.machine)?,
            "profiles" => serde_json::to_value(&scan.users)?,
            "applications" => serde_json::to_value(&scan.applications)?,
            "printers" => serde_json::to_value(&self.manifest.printers)?,
            "network-drives" => serde_json::to_value(&self.manifest.mapped_drives)?,
            "outlook" => {
                let per_user: Vec<serde_json::Value> = items
                    .iter()
                    .filter_map(|i| i.owner.as_ref())
                    .map(|o| {
                        let data_files: Vec<serde_json::Value> = scan
                            .items
                            .iter()
                            .filter(|x| x.category == Category::OutlookEmail && x.owner.as_ref().is_some_and(|w| w.sid == o.sid))
                            .map(|x| serde_json::json!({"name": x.display_name, "source": x.source_display(), "size": x.estimated_size, "support": x.support}))
                            .collect();
                        serde_json::json!({
                            "account": o.account_name,
                            "sid": o.sid,
                            "profile_names": scan.items.iter().find(|x| ids.contains(x.id.as_str()) && x.owner.as_ref().is_some_and(|w| w.sid == o.sid)).map(|x| x.description.clone()),
                            "discovered_outlook_data": data_files,
                            "recommended_restore_procedure": [
                                "Install Outlook and sign in; Autodiscover recreates the mail profile.",
                                "Signatures, templates and stationery are restored as files by Migration Assistant.",
                                "Open restored PST files via File > Open & Export > Open Outlook Data File.",
                                "OST files are not migrated; mail re-synchronizes from the server."
                            ],
                            "not_included": ["Passwords", "Tokens", "Server account settings"]
                        })
                    })
                    .collect();
                serde_json::Value::Array(per_user)
            }
            other => serde_json::json!({ "inventory": other }),
        };
        crate::util::write_json_atomic(&path, &value)?;
        let mut entries = vec![HashEntry { sha256: hashing::sha256_file(&path)?, path: rel.clone() }];
        if name == "applications" {
            let checklist_rel = "inventory/reinstall-checklist.txt";
            let mut text = String::from("Recommended reinstall checklist (generated by Migration Assistant)\n\nInstall from official sources. Licenses and product keys are not migrated.\n\n");
            for line in crate::discovery::apps::reinstall_checklist(&scan.applications) {
                text.push_str(&line);
                text.push('\n');
            }
            let p = self.layout.resolve(checklist_rel)?;
            crate::util::write_bytes_atomic(&p, text.as_bytes())?;
            entries.push(HashEntry { sha256: hashing::sha256_file(&p)?, path: checklist_rel.into() });
        }
        tracker.set_totals(None, Some(entries.len() as u64));
        tracker.advance(0, entries.len() as u64, Some(&rel));
        tracker.set_state(TaskState::Verifying);
        let list_rel = format!("hashes/{}.sha256", task_key(task_id));
        let digest = hashing::write_hash_list(&self.layout.resolve(&list_rel)?, &entries)?;
        // Hash list is attached to the first item; others reference the same file.
        for (n, m) in self.manifest.items.iter_mut().filter(|m| ids.contains(m.id.as_str())).enumerate() {
            m.capture_status = CaptureStatus::Captured;
            m.hash_status = HashStatus::Verified;
            m.captured_files = if n == 0 { entries.len() as u64 } else { 0 };
            if n == 0 {
                m.hash_list = Some(list_rel.clone());
                m.hash_list_sha256 = Some(digest.clone());
            }
        }
        tracker.progress.current_path = None;
        tracker.set_state(TaskState::Completed);
        Ok(())
    }

    fn finish(mut self, canceled: bool) -> AppResult<CaptureSummary> {
        let digests: Vec<String> = self.manifest.items.iter().filter_map(|m| m.hash_list_sha256.clone()).collect();
        self.manifest.integrity.bundle_root_hash = Some(hashing::root_hash(digests.iter().map(String::as_str)));
        self.manifest.integrity.total_files = self.manifest.items.iter().map(|m| m.captured_files).sum();
        self.manifest.integrity.total_bytes = self.manifest.items.iter().map(|m| m.captured_bytes).sum();
        self.manifest.integrity.mismatches = self.manifest.items.iter().filter(|m| m.hash_status == HashStatus::Mismatch).count() as u64;
        let any_failed = self.manifest.items.iter().any(|m| m.capture_status == CaptureStatus::Failed);
        let any_warn = self.manifest.items.iter().any(|m| m.capture_status == CaptureStatus::CapturedWithWarnings);
        // A file that could not be captured (not merely skipped by policy) is an error.
        let any_file_error = self.manifest.items.iter().any(|m| m.warnings.iter().any(|w| w.severity == Severity::Error));

        let structural = if canceled {
            None
        } else {
            let v = bundle::verify_bundle(&self.layout, &self.manifest, false, &CancelToken::new(), |_| {})?;
            if !v.ok {
                self.logger.error(None, format!("Bundle verification failed: {} missing, {} mismatched, {} error(s)", v.missing.len(), v.mismatches.len(), v.errors.len()));
            }
            Some(v.ok)
        };
        let verified = structural == Some(true) && !any_failed && !any_file_error && self.manifest.integrity.mismatches == 0;
        self.manifest.integrity.verified = verified;
        self.manifest.integrity.verified_at = verified.then(now_utc);
        self.manifest.status = if canceled {
            BundleStatus::Canceled
        } else if !verified {
            BundleStatus::CompletedUnverified
        } else if any_warn {
            BundleStatus::VerifiedWithWarnings
        } else {
            BundleStatus::Verified
        };
        if !canceled {
            self.manifest.completed_at = Some(now_utc());
        }
        self.manifest.capacity.destination_free_bytes = self.platform.disk_space(&self.layout.root).map(|d| d.free_bytes).or(self.manifest.capacity.destination_free_bytes);
        self.logger.info(None, format!("Capture finished with status {:?}", self.manifest.status));
        self.manifest.log_summary = self.logger.summary("logs/capture.log.jsonl");
        self.logger.flush();
        bundle::write_manifest(&self.layout, &self.manifest)?;
        let reports = crate::reporting::write_capture_reports(&self.layout, &self.manifest, &self.results)?;
        let warnings: Vec<Warning> = self.manifest.items.iter().flat_map(|m| m.warnings.iter().filter(|w| w.severity >= Severity::Warning).cloned()).collect();
        Ok(CaptureSummary {
            bundle_id: self.manifest.bundle_id.clone(),
            bundle_path: display_path(&self.layout.root),
            machine_name: self.manifest.source_machine.computer_name.clone(),
            captured_users: self.manifest.users.iter().map(|u| u.account_name.clone()).collect(),
            total_bytes: self.manifest.integrity.total_bytes,
            total_files: self.manifest.integrity.total_files,
            verified,
            status: self.manifest.status,
            warnings,
            task_results: self.results,
            report_html: reports.technician_html,
            report_json: reports.json,
            summary_report_html: reports.summary_html,
        })
    }
}
