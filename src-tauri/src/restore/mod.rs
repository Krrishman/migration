//! Restore engine: validate → map users → dry-run plan → confirmed,
//! dependency-ordered execution → verification → report → manifest history.
//!
//! Safety rules enforced here:
//! * Bundle integrity is verified before options are shown and again before writing.
//! * Nothing is written for a category the technician has not confirmed.
//! * Existing files are never overwritten silently: skip (default), rename the
//!   incoming file, or (with an extra per-category confirmation) move the
//!   existing file to a `.bak` copy first.
//! * Drive mappings, printers and the wallpaper registry value are listed
//!   verbatim in the plan and applied only after confirmation.

pub mod bookmarks;
pub mod files;

use crate::bundle::{self, BundleLayout};
use crate::capture::hashing;
use crate::discovery::browsers::default_providers;
use crate::discovery::item_id;
use crate::discovery::personalization::{STICKY_NOTES_PACKAGE, STICKY_NOTES_PROCESSES};
use crate::error::{AppError, AppResult};
use crate::models::*;
use crate::platform::Platform;
use crate::progress::{Logger, ProgressSink, TaskTracker};
use crate::security::encryption::{self, BundleKey, ENCRYPTED_EXTENSION};
use crate::security::safe_path::{display_path, join_within, safe_relative};
use crate::util::{format_bytes, now_utc, CancelToken};
use files::{resolve, write_file, Resolution};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct OpenedBundle {
    pub layout: BundleLayout,
    pub manifest: Manifest,
    pub validation: BundleValidation,
}

/// A payload file inside the bundle.
#[derive(Debug, Clone)]
pub struct BundleFile {
    pub stored_rel: String,
    /// Path relative to the item's root, without the encryption suffix.
    pub rel: String,
    pub plain_hash: String,
    pub size: u64,
}

enum Op {
    CopyTree { item: ManifestItem, files: Vec<BundleFile>, target_dir: PathBuf },
    Bookmarks { browser: BrowserKind, source: BundleFile, out_file: PathBuf, title: String },
    Wallpaper { item: ManifestItem, files: Vec<BundleFile>, target_dir: PathBuf, apply: bool },
    MapDrive(MappedDrive),
    SharedPrinter(String),
    NetworkPrinter { name: String, host: String, driver: String, port: String },
    Manual,
}

pub struct RestoreService<'a> {
    pub platform: &'a dyn Platform,
    pub sink: Arc<dyn ProgressSink>,
    pub cancel: CancelToken,
}

/// Read the item's hash lists and return its payload files.
pub fn item_files(layout: &BundleLayout, manifest: &Manifest, item: &ManifestItem) -> AppResult<Vec<BundleFile>> {
    let Some(list) = &item.hash_list else { return Ok(vec![]) };
    let text = std::fs::read_to_string(layout.resolve(list)?).map_err(|e| AppError::io(list, e))?;
    if let Some(expected) = &item.hash_list_sha256 {
        if !hashing::sha256_bytes(text.as_bytes()).eq_ignore_ascii_case(expected) {
            return Err(AppError::Integrity(format!("hash list {list} was modified")));
        }
    }
    let entries = hashing::parse_hash_list(&text)?;
    let plain: HashMap<String, String> = if manifest.encryption.enabled {
        let plain_rel = list.trim_end_matches(".sha256").to_string() + ".plain.sha256";
        let t = std::fs::read_to_string(layout.resolve(&plain_rel)?).map_err(|e| AppError::io(&plain_rel, e))?;
        hashing::parse_hash_list(&t)?.into_iter().map(|e| (e.path, e.sha256)).collect()
    } else {
        HashMap::new()
    };
    let prefix = format!("{}/", item.bundle_path);
    let suffix = format!(".{ENCRYPTED_EXTENSION}");
    let mut out = Vec::new();
    for e in entries {
        let Some(rest) = e.path.strip_prefix(&prefix) else { continue };
        let rel = if manifest.encryption.enabled { rest.strip_suffix(&suffix).unwrap_or(rest).to_string() } else { rest.to_string() };
        safe_relative(&rel)?;
        let size = std::fs::metadata(layout.resolve(&e.path)?).map(|m| m.len()).unwrap_or(0);
        let plain_hash = if manifest.encryption.enabled {
            plain.get(&e.path).cloned().ok_or_else(|| AppError::Integrity(format!("no plaintext hash for {}", e.path)))?
        } else {
            e.sha256.clone()
        };
        out.push(BundleFile { stored_rel: e.path, rel, plain_hash, size });
    }
    Ok(out)
}

/// Suggest mappings: same short account name or display name, else the
/// current target user when the bundle holds exactly one user.
pub fn default_mappings(manifest: &Manifest, targets: &[UserProfile]) -> Vec<UserMapping> {
    let short = |a: &str| a.rsplit('\\').next().unwrap_or(a).to_lowercase();
    let candidates: Vec<&UserProfile> = targets.iter().filter(|t| !t.is_system_account && t.profile_exists).collect();
    manifest
        .users
        .iter()
        .map(|u| {
            let by_name = candidates.iter().find(|t| short(&t.account_name) == short(&u.account_name));
            let by_display = candidates.iter().find(|t| t.display_name.is_some() && t.display_name == u.display_name);
            let fallback = if manifest.users.len() == 1 { candidates.iter().find(|t| t.is_current_user) } else { None };
            UserMapping { source_sid: u.sid.clone(), target_sid: by_name.or(by_display).or(fallback).map(|t| t.sid.clone()) }
        })
        .collect()
}

fn roaming(platform: &dyn Platform, u: &UserProfile) -> PathBuf {
    platform.roaming_app_data(u)
}
fn local(platform: &dyn Platform, u: &UserProfile) -> PathBuf {
    platform.local_app_data(u)
}
fn push_rel(mut base: PathBuf, rel: &str) -> PathBuf {
    for p in rel.split(['/', '\\']).filter(|p| !p.is_empty()) {
        base.push(p);
    }
    base
}

/// Map a profile-relative path onto the target user, honouring AppData roots.
fn map_profile_relative(platform: &dyn Platform, tp: &UserProfile, rel: &str) -> PathBuf {
    let lower = rel.to_lowercase();
    if let Some(r) = lower.strip_prefix("appdata/roaming/") {
        return push_rel(roaming(platform, tp), &rel[rel.len() - r.len()..]);
    }
    if let Some(r) = lower.strip_prefix("appdata/local/") {
        return push_rel(local(platform, tp), &rel[rel.len() - r.len()..]);
    }
    push_rel(tp.profile_path.clone(), rel)
}

fn last_component(p: &str) -> String {
    p.rsplit(['/', '\\']).find(|s| !s.is_empty()).unwrap_or("Folder").to_string()
}

fn migrated(tp: &UserProfile, sub: &str) -> PathBuf {
    push_rel(tp.profile_path.join("Migrated Files"), sub)
}

impl<'a> RestoreService<'a> {
    /// Open, validate and (optionally fully) verify a bundle.
    pub fn open_bundle(&self, path: &Path, full_verify: bool, on_file: impl FnMut(&str)) -> AppResult<OpenedBundle> {
        let layout = BundleLayout::open(path)?;
        let (manifest, hash_ok) = bundle::read_manifest(&layout)?;
        let mut validation = bundle::verify_bundle(&layout, &manifest, full_verify, &self.cancel, on_file)?;
        validation.manifest_hash_ok = hash_ok;
        if !hash_ok {
            validation.errors.push("manifest.json does not match manifest.sha256 (the manifest was modified after capture)".into());
            validation.ok = false;
        }
        if !matches!(manifest.status, BundleStatus::Verified | BundleStatus::VerifiedWithWarnings) {
            validation.errors.push(format!("Bundle capture status is {:?}; only verified bundles can be restored.", manifest.status));
            validation.ok = false;
        }
        Ok(OpenedBundle { layout, manifest, validation })
    }

    pub fn target_info(&self) -> AppResult<TargetInfo> {
        let m = self.platform.machine_info()?;
        let profiles = self.platform.list_profiles()?;
        let free = profiles.iter().find(|p| p.is_current_user).and_then(|p| self.platform.disk_space(&p.profile_path)).map(|d| d.free_bytes);
        Ok(TargetInfo {
            computer_name: m.computer_name,
            os_name: m.os_name,
            os_version: m.os_version,
            elevated: self.platform.is_elevated(),
            profiles,
            running_processes: self.platform.running_processes().into_iter().map(|p| p.name).collect(),
            free_bytes_system_drive: free,
        })
    }

    fn running(&self, names: &[&str]) -> Vec<String> {
        let procs = self.platform.running_processes();
        let mut v: Vec<String> = procs.into_iter().filter(|p| names.iter().any(|n| p.name.eq_ignore_ascii_case(n))).map(|p| p.name).collect();
        v.sort();
        v.dedup();
        v
    }

    /// Build a dry-run plan. Writes nothing.
    pub fn plan(&self, opened: &OpenedBundle, req: &RestoreRequest) -> AppResult<RestorePlan> {
        Ok(self.build(opened, req)?.0)
    }

    fn build(&self, opened: &OpenedBundle, req: &RestoreRequest) -> AppResult<(RestorePlan, Vec<Op>)> {
        let m = &opened.manifest;
        let targets = self.platform.list_profiles()?;
        let elevated = self.platform.is_elevated();
        let current = targets.iter().find(|t| t.is_current_user).cloned();
        let mut mapping: HashMap<&str, &UserProfile> = HashMap::new();
        for mp in &req.mappings {
            if let Some(t) = &mp.target_sid {
                let tp = targets
                    .iter()
                    .find(|p| &p.sid == t && !p.is_system_account)
                    .ok_or_else(|| AppError::InvalidRequest(format!("destination user {t} was not found on this PC")))?;
                mapping.insert(mp.source_sid.as_str(), tp);
            }
        }
        let selected: Vec<&ManifestItem> = m
            .items
            .iter()
            .filter(|i| req.selected_item_ids.contains(&i.id))
            .filter(|i| matches!(i.capture_status, CaptureStatus::Captured | CaptureStatus::CapturedWithWarnings))
            .collect();
        let policy = |c: Category| req.policies.get(&c).copied().unwrap_or_default();
        let mut actions: Vec<(RestoreAction, Op)> = Vec::new();
        let mut warnings = Vec::new();

        for item in selected {
            let tp: Option<&UserProfile> = match &item.owner {
                Some(o) => match mapping.get(o.sid.as_str()) {
                    Some(t) => Some(*t),
                    None => continue, // user not mapped: not restored
                },
                None => current.as_ref(),
            };
            let mut action = RestoreAction {
                id: format!("restore:{}", item.id),
                item_id: item.id.clone(),
                category: item.category,
                display_name: item.display_name.clone(),
                source_user: item.owner.as_ref().map(|o| o.account_name.clone()),
                target_user: tp.map(|t| t.account_name.clone()),
                target_path: None,
                files: 0,
                bytes: item.captured_bytes,
                conflicts: 0,
                policy: policy(item.category),
                requires_admin: false,
                requires_confirmation: true,
                system_changes: vec![],
                blocked_reason: None,
                warnings: vec![],
                notes: item.restore_notes.clone(),
            };
            let Some(tp) = tp else {
                action.blocked_reason = Some("No destination user is available.".into());
                actions.push((action, Op::Manual));
                continue;
            };
            if !tp.is_current_user && !elevated {
                action.requires_admin = true;
                action.blocked_reason =
                    Some(format!("Writing into {}'s profile requires administrator rights. Restart elevated or sign in as that user.", tp.account_name));
            }

            let target_dir: Option<PathBuf> = match &item.restore_kind {
                RestoreKind::KnownFolder { folder } => Some(self.platform.known_folder(tp, *folder)),
                RestoreKind::CustomFolder => Some(match &item.profile_relative {
                    Some(rel) if !rel.is_empty() => map_profile_relative(self.platform, tp, rel),
                    _ => migrated(tp, &last_component(&item.source_path)),
                }),
                RestoreKind::OneDriveLocal => Some(migrated(tp, &format!("OneDrive/{}", last_component(&item.source_path)))),
                RestoreKind::PublicDesktop => {
                    let pd = self.platform.public_desktop();
                    if !elevated {
                        action.requires_admin = true;
                        action.blocked_reason = Some("The Public Desktop is shared by all users; restoring it requires administrator rights.".into());
                    }
                    pd
                }
                RestoreKind::StartMenuShortcuts => Some(push_rel(roaming(self.platform, tp), "Microsoft/Windows/Start Menu/Programs")),
                RestoreKind::TaskbarPins => Some(migrated(tp, "Taskbar shortcuts")),
                RestoreKind::QuickAccess => Some(push_rel(roaming(self.platform, tp), "Microsoft/Windows/Recent/AutomaticDestinations")),
                RestoreKind::RecentItems => Some(migrated(tp, "Recent items")),
                RestoreKind::OutlookSignatures => Some(push_rel(roaming(self.platform, tp), "Microsoft/Signatures")),
                RestoreKind::OutlookTemplates => Some(push_rel(roaming(self.platform, tp), "Microsoft/Templates")),
                RestoreKind::OutlookStationery => Some(push_rel(roaming(self.platform, tp), "Microsoft/Stationery")),
                RestoreKind::PstFile => Some(self.platform.known_folder(tp, KnownFolder::Documents).join("Outlook Files")),
                RestoreKind::OfficeTemplates => item.profile_relative.as_ref().map(|rel| map_profile_relative(self.platform, tp, rel)),
                RestoreKind::Wallpaper => Some(self.platform.known_folder(tp, KnownFolder::Pictures).join("Migrated Wallpaper")),
                RestoreKind::Themes => Some(push_rel(local(self.platform, tp), "Microsoft/Windows/Themes")),
                RestoreKind::StickyNotes => {
                    let pkg = push_rel(local(self.platform, tp), &format!("Packages/{STICKY_NOTES_PACKAGE}"));
                    if pkg.is_dir() {
                        let running = self.running(STICKY_NOTES_PROCESSES);
                        if !running.is_empty() {
                            action.blocked_reason = Some("Sticky Notes is running on this PC. Close it and refresh the plan.".into());
                        }
                        Some(pkg.join("LocalState"))
                    } else {
                        action.notes.push("Sticky Notes is not installed for this user; the database is placed in Migrated Files for manual use.".into());
                        Some(migrated(tp, "Sticky Notes"))
                    }
                }
                RestoreKind::BrowserProfile { browser, profile_dir } => {
                    let files = item_files(&opened.layout, m, item)?;
                    // 1) Always-safe bookmarks export to the desktop.
                    let bm_name = if *browser == BrowserKind::Firefox { "places.sqlite" } else { "Bookmarks" };
                    if let Some(src) = files.iter().find(|f| f.rel == bm_name) {
                        let desktop = self.platform.known_folder(tp, KnownFolder::Desktop).join("Migrated Browser Data");
                        let base =
                            desktop.join(format!("{} - {} bookmarks.html", browser.label(), crate::security::safe_path::sanitize_component(profile_dir)));
                        let out = match resolve(&base, CollisionPolicy::RenameIncoming, false, "")? {
                            Resolution::Write(p) => p,
                            _ => base.clone(),
                        };
                        let bm = RestoreAction {
                            id: format!("bookmarks:{}", item.id),
                            display_name: format!("{} – bookmarks export (HTML)", item.display_name),
                            target_path: Some(display_path(&out)),
                            files: 1,
                            bytes: src.size,
                            conflicts: 0,
                            policy: CollisionPolicy::RenameIncoming,
                            system_changes: vec![],
                            blocked_reason: action.blocked_reason.clone(),
                            notes: vec!["Creates an importable bookmarks file; existing browser data is not touched.".into()],
                            ..action.clone()
                        };
                        actions.push((bm, Op::Bookmarks { browser: *browser, source: src.clone(), out_file: out, title: item.display_name.clone() }));
                    }
                    // 2) Optional profile-file restore.
                    let procs: Vec<&'static str> =
                        default_providers().iter().filter(|p| p.kind() == *browser).flat_map(|p| p.process_names().to_vec()).collect();
                    let running = self.running(&procs);
                    if !running.is_empty() && action.blocked_reason.is_none() {
                        action.blocked_reason =
                            Some(format!("{} is running on this PC ({}). Close it and refresh the plan.", browser.label(), running.join(", ")));
                    }
                    action.warnings.push(Warning::info(
                        WarningCode::CredentialsNotMigrated,
                        "Passwords, cookies and sign-ins were never captured; sign in to browser sync to restore them.",
                    ));
                    Some(match browser {
                        BrowserKind::Chrome => push_rel(local(self.platform, tp), &format!("Google/Chrome/User Data/{profile_dir}")),
                        BrowserKind::Edge => push_rel(local(self.platform, tp), &format!("Microsoft/Edge/User Data/{profile_dir}")),
                        BrowserKind::Firefox | BrowserKind::Chromium => {
                            action.notes.push("Profile files are placed in Migrated Files. In Firefox, open about:profiles > Create a New Profile > Choose Folder to use them.".into());
                            migrated(tp, &format!("{} profile {}", browser.label(), profile_dir))
                        }
                    })
                }
                RestoreKind::MappedDrives => {
                    let existing = self.platform.mapped_drives().unwrap_or_default();
                    for d in m.mapped_drives.iter().filter(|d| item_id("drive", None, &d.letter) == item.id) {
                        action.target_path = Some(format!("{} → {}", d.letter, d.unc_path));
                        action.system_changes.push(SystemChange::MapDrive { letter: d.letter.clone(), unc_path: d.unc_path.clone(), persistent: d.persistent });
                        if let Some(e) = existing.iter().find(|e| e.letter.eq_ignore_ascii_case(&d.letter)) {
                            action.conflicts = 1;
                            action.blocked_reason = Some(format!("{} is already mapped to {} on this PC; it is left unchanged.", d.letter, e.unc_path));
                        }
                        actions.push((action.clone(), Op::MapDrive(d.clone())));
                    }
                    continue;
                }
                RestoreKind::Printers => {
                    for p in m.printers.iter().filter(|p| item_id("printer", None, &p.name) == item.id) {
                        let mut a = action.clone();
                        a.id = format!("printer:{}", item.id);
                        let op = match p.connection {
                            PrinterConnection::Shared => {
                                let unc = p.unc_path.clone().unwrap_or_else(|| p.name.clone());
                                a.target_path = Some(unc.clone());
                                a.system_changes.push(SystemChange::ConnectSharedPrinter { unc_path: unc.clone() });
                                Op::SharedPrinter(unc)
                            }
                            PrinterConnection::Network if p.host_address.is_some() => {
                                let host = p.host_address.clone().unwrap_or_default();
                                let port = format!("IP_{host}");
                                match self.platform.printer_driver_installed(&p.driver_name) {
                                    Ok(true) => {
                                        a.requires_admin = true;
                                        if !elevated {
                                            a.blocked_reason = Some("Adding a TCP/IP printer requires administrator rights.".into());
                                        }
                                        a.target_path = Some(format!("{host} ({})", p.driver_name));
                                        a.system_changes.push(SystemChange::AddNetworkPrinter {
                                            name: p.name.clone(),
                                            host_address: host.clone(),
                                            driver_name: p.driver_name.clone(),
                                            port_name: port.clone(),
                                        });
                                        Op::NetworkPrinter { name: p.name.clone(), host, driver: p.driver_name.clone(), port }
                                    }
                                    other => {
                                        if let Err(e) = other {
                                            a.warnings.push(Warning::warn(WarningCode::AdapterUnavailable, format!("Could not check drivers: {e}")));
                                        }
                                        a.warnings.push(Warning::warn(
                                            WarningCode::DriverRequired,
                                            format!("Driver \"{}\" is not installed on this PC. Drivers are never installed automatically.", p.driver_name),
                                        ));
                                        a.system_changes.push(SystemChange::ManualChecklist {
                                            text: format!("Install \"{}\" from the manufacturer, then add printer {} at {host}.", p.driver_name, p.name),
                                        });
                                        Op::Manual
                                    }
                                }
                            }
                            PrinterConnection::Virtual => {
                                a.notes.push("Built-in virtual printer; nothing to restore.".into());
                                Op::Manual
                            }
                            _ => {
                                a.system_changes.push(SystemChange::ManualChecklist {
                                    text: format!(
                                        "Connect \"{}\" ({:?}) and install driver \"{}\" from the manufacturer; print a test page.",
                                        p.name, p.connection, p.driver_name
                                    ),
                                });
                                Op::Manual
                            }
                        };
                        actions.push((a, op));
                    }
                    continue;
                }
                RestoreKind::Inventory { .. } => {
                    action.requires_confirmation = false;
                    action.notes.insert(0, "Inventory only: included in reports, nothing is written to this PC.".into());
                    actions.push((action, Op::Manual));
                    continue;
                }
            };

            let Some(target_dir) = target_dir else {
                action.blocked_reason = Some("No restore location is defined for this item on this PC.".into());
                actions.push((action, Op::Manual));
                continue;
            };
            let files = item_files(&opened.layout, m, item)?;
            action.files = files.len() as u64;
            action.bytes = item.captured_bytes;
            action.conflicts = files.iter().filter(|f| push_rel(target_dir.clone(), &f.rel).exists()).count() as u64;
            action.target_path = Some(display_path(&target_dir));
            if action.conflicts > 0 {
                action.warnings.push(Warning::warn(
                    WarningCode::Conflict,
                    format!("{} file(s) already exist at the destination; policy: {:?}.", action.conflicts, action.policy),
                ));
            }
            if item.restore_kind == RestoreKind::Wallpaper {
                let apply = tp.is_current_user;
                if apply {
                    if let Some(f) = files.first() {
                        let final_path = push_rel(target_dir.clone(), &f.rel);
                        action.system_changes.push(SystemChange::RegistryValue {
                            hive: "HKCU".into(),
                            key: r"Control Panel\Desktop".into(),
                            value_name: "WallPaper".into(),
                            value: display_path(&final_path),
                        });
                    }
                } else {
                    action.notes.push("The wallpaper image is copied; sign in as that user to apply it.".into());
                }
                actions.push((action, Op::Wallpaper { item: item.clone(), files, target_dir, apply }));
            } else {
                actions.push((action, Op::CopyTree { item: item.clone(), files, target_dir }));
            }
        }

        // Dependency-aware order; bookmark exports come before profile restores.
        actions.sort_by(|(a, _), (b, _)| {
            a.category.restore_order().cmp(&b.category.restore_order()).then_with(|| b.id.starts_with("bookmarks:").cmp(&a.id.starts_with("bookmarks:")))
        });
        let total_bytes: u64 = actions.iter().filter(|(a, _)| a.blocked_reason.is_none()).map(|(a, _)| a.bytes).sum();
        let target_free = current.as_ref().and_then(|c| self.platform.disk_space(&c.profile_path)).map(|d| d.free_bytes);
        if let Some(free) = target_free {
            if free < total_bytes + total_bytes / 20 {
                warnings.push(Warning::new(
                    WarningCode::LowDiskSpace,
                    Severity::Error,
                    format!("Not enough free space: about {} needed, {} available.", format_bytes(total_bytes), format_bytes(free)),
                ));
            }
        }
        if m.encryption.enabled && req.passphrase.as_deref().unwrap_or("").is_empty() {
            warnings.push(Warning::warn(WarningCode::Other, "This bundle is encrypted. The passphrase is required to restore files."));
        }
        let plan = RestorePlan {
            plan_id: uuid::Uuid::new_v4().to_string(),
            bundle_id: m.bundle_id.clone(),
            dry_run: true,
            total_files: actions.iter().map(|(a, _)| a.files).sum(),
            total_conflicts: actions.iter().map(|(a, _)| a.conflicts).sum(),
            total_bytes,
            target_free_bytes: target_free,
            warnings,
            actions: actions.iter().map(|(a, _)| a.clone()).collect(),
        };
        Ok((plan, actions.into_iter().map(|(_, o)| o).collect()))
    }

    /// Execute a confirmed restore.
    pub fn execute(&self, opened: &mut OpenedBundle, req: &RestoreRequest) -> AppResult<RestoreSummary> {
        let started = now_utc();
        // Integrity gate, re-checked immediately before writing.
        let quick = bundle::verify_bundle(&opened.layout, &opened.manifest, false, &self.cancel, |_| {})?;
        if !opened.validation.ok || !quick.ok {
            return Err(AppError::Integrity("bundle verification failed; restore is not allowed".into()));
        }
        let key: Option<BundleKey> = if opened.manifest.encryption.enabled {
            let p =
                req.passphrase.as_deref().filter(|p| !p.is_empty()).ok_or_else(|| AppError::Crypto("this bundle is encrypted; enter its passphrase".into()))?;
            Some(encryption::unlock_bundle(&opened.manifest.encryption, p)?)
        } else {
            None
        };
        let (mut plan, ops) = self.build(opened, req)?;
        plan.dry_run = false;
        if plan.warnings.iter().any(|w| w.severity == Severity::Error) {
            return Err(AppError::InvalidRequest(
                plan.warnings.iter().filter(|w| w.severity == Severity::Error).map(|w| w.message.clone()).collect::<Vec<_>>().join(" "),
            ));
        }
        let restore_id = uuid::Uuid::new_v4().to_string();
        let stamp = started.format("%Y%m%d%H%M%S").to_string();
        let log_path = opened.layout.logs().join(format!("restore-{}.log.jsonl", &restore_id[..8]));
        let logger = Logger::new(&log_path, self.sink.clone());
        let target_name = self.platform.machine_info().map(|m| m.computer_name).unwrap_or_else(|_| "unknown".into());
        logger.info(None, format!("Restoring bundle {} onto {target_name} (platform: {})", opened.manifest.bundle_id, self.platform.name()));

        let mut trackers: Vec<TaskTracker> = plan.actions.iter().map(|a| TaskTracker::new(&a.id, a.category, &a.display_name, self.sink.clone())).collect();
        for t in &mut trackers {
            t.emit(true);
        }
        let mut s = RestoreSummary {
            restore_id: restore_id.clone(),
            bundle_id: opened.manifest.bundle_id.clone(),
            files_written: 0,
            files_skipped: 0,
            files_renamed: 0,
            files_replaced: 0,
            failures: 0,
            verified: true,
            outcome: String::new(),
            task_results: vec![],
            warnings: vec![],
            report_html: String::new(),
        };
        let mut restored_items = Vec::new();
        for ((action, op), mut t) in plan.actions.iter().zip(ops).zip(trackers) {
            t.start();
            if self.cancel.is_canceled() {
                t.set_state(TaskState::Canceled);
                s.task_results.push(t.progress);
                continue;
            }
            if let Some(reason) = &action.blocked_reason {
                t.progress.error = Some(reason.clone());
                t.set_state(TaskState::Skipped);
                s.warnings.push(Warning::warn(WarningCode::Skipped, format!("{}: {reason}", action.display_name)));
                logger.warn(Some(&action.id), format!("Skipped: {reason}"));
                s.task_results.push(t.progress);
                continue;
            }
            if action.requires_confirmation && !req.confirmed_categories.contains(&action.category) {
                t.progress.error = Some("Not confirmed".into());
                t.set_state(TaskState::Skipped);
                s.warnings.push(Warning::warn(WarningCode::Skipped, format!("{}: not confirmed by the technician; nothing written.", action.display_name)));
                s.task_results.push(t.progress);
                continue;
            }
            let replace_ok = req.replace_confirmed.contains(&action.category);
            let result = match op {
                Op::CopyTree { item, files, target_dir } => {
                    self.copy_tree(&opened.layout, &item, &files, &target_dir, action.policy, replace_ok, &stamp, key.as_ref(), &mut t, &mut s, &logger)
                }
                Op::Wallpaper { item, files, target_dir, apply } => {
                    let r =
                        self.copy_tree(&opened.layout, &item, &files, &target_dir, action.policy, replace_ok, &stamp, key.as_ref(), &mut t, &mut s, &logger);
                    match (r, apply, files.first()) {
                        (Ok(()), true, Some(f)) => {
                            let p = push_rel(target_dir, &f.rel);
                            if p.is_file() {
                                self.platform.set_wallpaper(&p).map(|_| logger.info(Some(&action.id), format!("Wallpaper set to {}", display_path(&p))))
                            } else {
                                Ok(())
                            }
                        }
                        (r, _, _) => r,
                    }
                }
                Op::Bookmarks { browser, source, out_file, title } => {
                    self.export_bookmarks(&opened.layout, browser, &source, &out_file, &title, key.as_ref(), &mut t, &mut s)
                }
                Op::MapDrive(d) => self
                    .platform
                    .map_drive(&d.letter, &d.unc_path, d.persistent)
                    .map(|_| logger.info(Some(&action.id), format!("Mapped {} to {}", d.letter, d.unc_path))),
                Op::SharedPrinter(unc) => self.platform.connect_shared_printer(&unc).map(|_| logger.info(Some(&action.id), format!("Connected {unc}"))),
                Op::NetworkPrinter { name, host, driver, port } => {
                    self.platform.add_network_printer(&name, &host, &driver, &port).map(|_| logger.info(Some(&action.id), format!("Added printer {name}")))
                }
                Op::Manual => Ok(()),
            };
            match result {
                Ok(()) => {
                    let warned = t.progress.warning_count > 0 || action.system_changes.iter().any(|c| matches!(c, SystemChange::ManualChecklist { .. }));
                    t.progress.current_path = None;
                    t.set_state(if warned { TaskState::CompletedWithWarnings } else { TaskState::Completed });
                    restored_items.push(action.item_id.clone());
                }
                Err(AppError::Canceled) => t.set_state(TaskState::Canceled),
                Err(e) => {
                    s.failures += 1;
                    t.progress.error = Some(e.to_string());
                    t.set_state(TaskState::Failed);
                    logger.error(Some(&action.id), format!("{}: {e}", action.display_name));
                    s.warnings.push(Warning::new(WarningCode::Other, Severity::Error, format!("{}: {e}", action.display_name)));
                }
            }
            s.task_results.push(t.progress);
        }
        s.verified = s.failures == 0 && !s.warnings.iter().any(|w| w.code == WarningCode::HashMismatch);
        s.outcome = if self.cancel.is_canceled() {
            "Canceled".into()
        } else if s.failures > 0 {
            "Completed with failures".into()
        } else if s.task_results.iter().any(|t| matches!(t.state, TaskState::CompletedWithWarnings | TaskState::Skipped)) {
            "Completed with warnings".into()
        } else {
            "Completed".into()
        };
        logger.info(None, format!("Restore finished: {}", s.outcome));
        logger.flush();

        // Report: inside the bundle when writable, otherwise in the target user's Documents.
        let html = crate::reporting::restore_report_html(&opened.manifest, &plan, &s, &target_name);
        let name = format!("restore-report-{}.html", &restore_id[..8]);
        let in_bundle = opened.layout.logs().join(&name);
        let report_path = match crate::util::write_bytes_atomic(&in_bundle, html.as_bytes()) {
            Ok(()) => in_bundle,
            Err(_) => {
                let current = self.platform.list_profiles()?.into_iter().find(|p| p.is_current_user);
                let dir = current
                    .map(|c| self.platform.known_folder(&c, KnownFolder::Documents).join("Migration Assistant Reports"))
                    .ok_or_else(|| AppError::InvalidRequest("no writable location for the restore report".into()))?;
                std::fs::create_dir_all(&dir).map_err(|e| AppError::io(&dir, e))?;
                let p = dir.join(&name);
                crate::util::write_bytes_atomic(&p, html.as_bytes())?;
                s.warnings.push(Warning::info(WarningCode::Other, format!("The bundle is read-only; the restore report was saved to {}", display_path(&p))));
                p
            }
        };
        s.report_html = display_path(&report_path);
        let event = RestoreEvent {
            restore_id,
            started_at: started,
            finished_at: now_utc(),
            target_computer: target_name,
            app_version: crate::APP_VERSION.into(),
            user_mappings: req.mappings.iter().filter_map(|m| m.target_sid.clone().map(|t| (m.source_sid.clone(), t))).collect(),
            restored_items,
            files_written: s.files_written,
            files_skipped: s.files_skipped,
            failures: s.failures,
            outcome: s.outcome.clone(),
            report_path: Some(s.report_html.clone()),
        };
        opened.manifest.restore_history.push(event);
        if let Err(e) = bundle::write_manifest(&opened.layout, &opened.manifest) {
            s.warnings.push(Warning::info(WarningCode::Other, format!("Restore history could not be appended to the manifest (bundle read-only?): {e}")));
        }
        Ok(s)
    }

    #[allow(clippy::too_many_arguments)]
    fn copy_tree(
        &self,
        layout: &BundleLayout,
        item: &ManifestItem,
        files: &[BundleFile],
        target_dir: &Path,
        policy: CollisionPolicy,
        replace_ok: bool,
        stamp: &str,
        key: Option<&BundleKey>,
        t: &mut TaskTracker,
        s: &mut RestoreSummary,
        logger: &Logger,
    ) -> AppResult<()> {
        t.set_totals(Some(files.iter().map(|f| f.size).sum()), Some(files.len() as u64));
        t.set_state(TaskState::Copying);
        for f in files {
            self.cancel.check()?;
            let src = layout.resolve(&f.stored_rel)?;
            let target = join_within(target_dir, &f.rel)?;
            t.progress.current_path = Some(display_path(&target));
            let resolution = match resolve(&target, policy, replace_ok, stamp) {
                Ok(r) => r,
                Err(e) => {
                    s.failures += 1;
                    t.warn();
                    logger.error(Some(&item.id), format!("{}: {e}", display_path(&target)));
                    t.advance(f.size, 1, None);
                    continue;
                }
            };
            let (dest, backup) = match resolution {
                Resolution::Skip => {
                    s.files_skipped += 1;
                    t.advance(f.size, 1, None);
                    continue;
                }
                Resolution::Write(p) => {
                    if p != target {
                        s.files_renamed += 1;
                    }
                    (p, None)
                }
                Resolution::Replace { target, backup } => {
                    std::fs::rename(&target, &backup).map_err(|e| AppError::io(&target, e))?;
                    logger.info(Some(&item.id), format!("Existing file kept as {}", display_path(&backup)));
                    (target, Some(backup))
                }
            };
            let mut done = 0u64;
            let r = write_file(&src, &dest, key, Some(&f.plain_hash), &self.cancel, &mut |n| {
                done += n;
            });
            match r {
                Ok(_) => {
                    s.files_written += 1;
                    if backup.is_some() {
                        s.files_replaced += 1;
                    }
                }
                Err(AppError::Canceled) => {
                    if let Some(b) = backup {
                        let _ = std::fs::rename(&b, &dest);
                    }
                    return Err(AppError::Canceled);
                }
                Err(e) => {
                    if let Some(b) = backup {
                        // Put the original back; nothing is lost.
                        let _ = std::fs::rename(&b, &dest);
                    }
                    s.failures += 1;
                    t.warn();
                    if matches!(e, AppError::Integrity(_)) {
                        s.warnings.push(Warning::new(WarningCode::HashMismatch, Severity::Error, e.to_string()).with_path(f.stored_rel.clone()));
                    } else if files::is_locked(&e) {
                        s.warnings.push(Warning::warn(WarningCode::LockedFile, format!("Destination file is in use: {}", display_path(&dest))));
                    }
                    logger.error(Some(&item.id), format!("{}: {e}", display_path(&dest)));
                }
            }
            t.advance(f.size, 1, None);
        }
        t.set_state(TaskState::Verifying);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn export_bookmarks(
        &self,
        layout: &BundleLayout,
        browser: BrowserKind,
        src: &BundleFile,
        out_file: &Path,
        title: &str,
        key: Option<&BundleKey>,
        t: &mut TaskTracker,
        s: &mut RestoreSummary,
    ) -> AppResult<()> {
        t.set_totals(Some(src.size), Some(1));
        t.set_state(TaskState::Copying);
        let stored = layout.resolve(&src.stored_rel)?;
        // Materialize (and verify) the source into a private temp file first.
        let tmp_dir = private_temp_dir()?;
        let plain = tmp_dir.join(if browser == BrowserKind::Firefox { "places.sqlite" } else { "Bookmarks" });
        write_file(&stored, &plain, key, Some(&src.plain_hash), &self.cancel, &mut |_| {})?;
        let nodes = if browser == BrowserKind::Firefox {
            bookmarks::parse_firefox(&plain)
        } else {
            std::fs::read(&plain).map_err(|e| AppError::io(&plain, e)).and_then(|b| bookmarks::parse_chromium(&b))
        };
        let _ = std::fs::remove_dir_all(&tmp_dir);
        let nodes = nodes?;
        let (html, exported, skipped) = bookmarks::to_netscape_html(&format!("Bookmarks – {title}"), &nodes);
        if skipped > 0 {
            t.warn();
            s.warnings.push(Warning::info(WarningCode::Skipped, format!("{skipped} script/data bookmark(s) were not exported.")));
        }
        let final_path = match resolve(out_file, CollisionPolicy::RenameIncoming, false, "")? {
            Resolution::Write(p) => p,
            _ => unreachable!("rename policy never skips"),
        };
        if let Some(parent) = final_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
        }
        crate::util::write_bytes_atomic(&final_path, html.as_bytes())?;
        s.files_written += 1;
        t.advance(src.size, 1, Some(&format!("{} ({exported} bookmarks)", display_path(&final_path))));
        Ok(())
    }
}

/// Short-lived working folder on the target PC (removed right after use)
/// for decrypting a bookmarks database before conversion.
fn private_temp_dir() -> AppResult<PathBuf> {
    let dir = std::env::temp_dir().join(format!("migration-assistant-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).map_err(|e| AppError::io(&dir, e))?;
    Ok(dir)
}

/// Policy map helper for callers.
pub fn policies(pairs: &[(Category, CollisionPolicy)]) -> BTreeMap<Category, CollisionPolicy> {
    pairs.iter().copied().collect()
}
