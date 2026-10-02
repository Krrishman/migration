//! Discovery: staged, read-only scanning of the source PC.
//!
//! Each concern is a [`DiscoveryModule`]; the [`DiscoveryService`] runs them
//! in stages, emits progress, and assembles a [`ScanResult`]. Modules never
//! write anything and never read protected secrets.

pub mod apps;
pub mod browsers;
pub mod desktop;
pub mod folders;
pub mod inventory;
pub mod outlook;
pub mod personalization;
pub mod plugins;
pub mod users;

use crate::error::AppResult;
use crate::fs_walk::{scan_size, walk, AllowList, WalkEvent, WalkOptions};
use crate::models::*;
use crate::platform::Platform;
use crate::security::exclusions::ExclusionRules;
use crate::util::CancelToken;
use std::path::{Path, PathBuf};

pub struct DiscoveryContext<'a> {
    pub platform: &'a dyn Platform,
    pub rules: ExclusionRules,
    pub cancel: CancelToken,
    pub processes: Vec<ProcessInfo>,
    /// Compute folder sizes (disable for a fast inventory-only rescan).
    pub measure_sizes: bool,
}

impl DiscoveryContext<'_> {
    pub fn process_running(&self, names: &[&str]) -> Vec<String> {
        self.processes
            .iter()
            .filter(|p| names.iter().any(|n| p.name.eq_ignore_ascii_case(n)))
            .map(|p| p.name.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect()
    }
}

/// Mutable scan state shared by modules.
#[derive(Default)]
pub struct ScanAccumulator {
    pub items: Vec<DiscoveryItem>,
    pub printers: Vec<PrinterInfo>,
    pub mapped_drives: Vec<MappedDrive>,
    pub applications: Vec<InstalledApp>,
    pub browser_profiles: Vec<BrowserProfileInfo>,
    pub warnings: Vec<Warning>,
}

pub trait DiscoveryModule: Send + Sync {
    fn id(&self) -> &'static str;
    /// Stage label shown while scanning.
    fn stage(&self) -> &'static str;
    fn discover(&self, ctx: &DiscoveryContext, users: &[UserProfile], acc: &mut ScanAccumulator) -> AppResult<()>;
}

/// Users whose per-user data is offered (non-system, existing profiles).
pub fn eligible_users(users: &[UserProfile]) -> impl Iterator<Item = &UserProfile> {
    users.iter().filter(|u| !u.is_system_account && u.profile_exists && u.access != AccessState::AccessDenied)
}

/// Stable item id. Ids are deterministic so a rescan keeps selections and a
/// resumed capture maps checkpoints onto the same items.
pub fn item_id(module: &str, owner: Option<&UserProfile>, key: &str) -> String {
    let owner = owner.map(|u| u.sid.as_str()).unwrap_or("machine");
    let key: String = key.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '.' { c.to_ascii_lowercase() } else { '_' }).collect();
    format!("{module}:{owner}:{key}")
}

/// Baseline item with conservative defaults (not selected, unknown access).
#[allow(clippy::too_many_arguments)] // every field is required; a builder would only add noise
pub fn base_item(
    id: String,
    category: Category,
    display_name: impl Into<String>,
    description: impl Into<String>,
    source: SourceRef,
    owner: Option<&UserProfile>,
    restore_kind: RestoreKind,
    payload: ItemPayload,
) -> DiscoveryItem {
    DiscoveryItem {
        id,
        category,
        display_name: display_name.into(),
        description: description.into(),
        source,
        owner: owner.map(UserProfile::user_ref),
        estimated_size: None,
        item_count: None,
        access: AccessState::Unknown,
        support: SupportLevel::Supported,
        selected_by_default: false,
        sensitive: false,
        opt_in_only: false,
        requires_admin: false,
        warnings: Vec::new(),
        restore_notes: Vec::new(),
        includes: Vec::new(),
        excludes: Vec::new(),
        restore_kind,
        payload,
    }
}

pub fn path_source(p: &Path) -> SourceRef {
    SourceRef::Path { path: p.to_path_buf() }
}

/// Probe whether a root folder is readable by the current token.
pub fn probe_access(path: &Path) -> AccessState {
    if !path.exists() {
        return AccessState::NotFound;
    }
    if path.is_file() {
        return match std::fs::File::open(path) {
            Ok(_) => AccessState::Accessible,
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => AccessState::AccessDenied,
            Err(e) if e.raw_os_error() == Some(32) || e.raw_os_error() == Some(33) => AccessState::Locked,
            Err(_) => AccessState::Unknown,
        };
    }
    match std::fs::read_dir(path) {
        Ok(_) => AccessState::Accessible,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => AccessState::AccessDenied,
        Err(_) => AccessState::Unknown,
    }
}

/// Fill size, count, access and file-level warnings for an item by walking
/// its payload with the exclusion rules. Never opens file contents.
pub fn measure(item: &mut DiscoveryItem, ctx: &DiscoveryContext) -> AppResult<()> {
    let (root, allow, files): (Option<PathBuf>, Option<AllowList>, Vec<PathBuf>) = match &item.payload {
        ItemPayload::Folder { root } => (Some(root.clone()), None, vec![]),
        ItemPayload::AllowList { root, files, dirs } => (Some(root.clone()), Some(AllowList { files: files.clone(), dirs: dirs.clone() }), vec![]),
        ItemPayload::FilesByExtension { root, extensions } => {
            let mut found = Vec::new();
            if root.is_dir() {
                walk(root, &ctx.rules, None, WalkOptions { non_recursive: true }, &ctx.cancel, |ev| {
                    if let WalkEvent::File { path, .. } = ev {
                        if has_extension(path, extensions) {
                            found.push(path.to_path_buf());
                        }
                    }
                })?;
            }
            (None, None, found)
        }
        ItemPayload::Files { files } => (None, None, files.clone()),
        ItemPayload::Inventory { .. } | ItemPayload::None => {
            if item.access == AccessState::Unknown {
                item.access = AccessState::Accessible;
            }
            return Ok(());
        }
    };

    if let Some(root) = root {
        item.access = probe_access(&root);
        if item.access != AccessState::Accessible {
            if item.access == AccessState::AccessDenied {
                item.requires_admin = true;
                item.warnings.push(
                    Warning::warn(
                        WarningCode::AccessDenied,
                        "Access denied for the current account. Restart elevated to include it; permissions are never bypassed.",
                    )
                    .with_path(root.display().to_string()),
                );
            }
            return Ok(());
        }
        if !ctx.measure_sizes {
            return Ok(());
        }
        let s = scan_size(&root, &ctx.rules, allow.as_ref(), &ctx.cancel)?;
        item.estimated_size = Some(s.bytes);
        item.item_count = Some(s.files);
        if s.access_denied > 0 {
            item.access = AccessState::PartiallyAccessible;
            item.warnings
                .push(Warning::warn(WarningCode::AccessDenied, format!("{} folder(s) or file(s) could not be read and will be skipped.", s.access_denied)));
        }
        if s.cloud_placeholders > 0 {
            item.warnings.push(Warning::warn(
                WarningCode::CloudPlaceholder,
                format!("{} online-only cloud file(s) are not stored on this PC and will not be downloaded or captured. They remain available from the cloud service.", s.cloud_placeholders),
            ));
        }
        if s.efs_files > 0 {
            item.warnings.push(Warning::warn(
                WarningCode::EfsEncrypted,
                format!(
                    "{} EFS-encrypted file(s). They may not open on the destination PC without the user's EFS certificate (export it with certmgr).",
                    s.efs_files
                ),
            ));
        }
        if s.long_paths > 0 {
            item.warnings.push(Warning::warn(WarningCode::LongPath, format!("{} file(s) have paths longer than 260 characters.", s.long_paths)));
        }
        if s.sensitive_excluded > 0 {
            item.warnings.push(Warning::info(
                WarningCode::SensitiveExcluded,
                format!("{} protected item(s) (credentials, keys, cookies or tokens) are excluded automatically.", s.sensitive_excluded),
            ));
        }
        if s.links_skipped > 0 {
            item.warnings
                .push(Warning::info(WarningCode::ReparsePointSkipped, format!("{} shortcut-like link(s) or junction(s) are not followed.", s.links_skipped)));
        }
    } else {
        let mut total = 0u64;
        let mut count = 0u64;
        let mut denied = 0u64;
        for f in &files {
            match std::fs::metadata(f) {
                Ok(m) => {
                    total += m.len();
                    count += 1;
                }
                Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => denied += 1,
                Err(_) => {}
            }
        }
        item.estimated_size = Some(total);
        item.item_count = Some(count);
        item.access = if files.is_empty() {
            AccessState::NotFound
        } else if denied == files.len() as u64 {
            AccessState::AccessDenied
        } else if denied > 0 {
            AccessState::PartiallyAccessible
        } else {
            AccessState::Accessible
        };
    }
    Ok(())
}

pub fn has_extension(path: &Path, extensions: &[String]) -> bool {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    extensions.iter().any(|e| e.trim_start_matches('.').eq_ignore_ascii_case(&ext))
}

/// Default selection policy: only supported, accessible, non-sensitive,
/// non-opt-in items belonging to the current user (or machine inventory).
pub fn apply_default_selection(item: &mut DiscoveryItem, owner: Option<&UserProfile>) {
    let owner_ok = owner.map(|o| o.is_current_user).unwrap_or(true);
    item.selected_by_default = owner_ok
        && !item.sensitive
        && !item.opt_in_only
        && item.support.is_capturable()
        && matches!(item.access, AccessState::Accessible | AccessState::PartiallyAccessible)
        && !item.warnings.iter().any(|w| w.severity == Severity::Error);
}

pub type ProgressFn<'a> = dyn Fn(ScanProgress) + Send + Sync + 'a;

pub struct DiscoveryService {
    modules: Vec<Box<dyn DiscoveryModule>>,
}

impl Default for DiscoveryService {
    fn default() -> Self {
        Self::new()
    }
}

impl DiscoveryService {
    pub fn new() -> Self {
        Self {
            modules: vec![
                Box::new(inventory::SystemInventoryModule),
                Box::new(users::UserProfilesModule),
                Box::new(folders::UserFoldersModule),
                Box::new(desktop::DesktopModule),
                Box::new(browsers::BrowsersModule::default()),
                Box::new(outlook::OutlookModule),
                Box::new(personalization::PersonalizationModule),
                Box::new(inventory::PrintersModule),
                Box::new(inventory::NetworkDrivesModule),
                Box::new(apps::InstalledAppsModule),
                Box::new(plugins::AppSettingsModule::default()),
            ],
        }
    }

    pub fn scan(&self, platform: &dyn Platform, cancel: &CancelToken, measure_sizes: bool, progress: &ProgressFn) -> AppResult<ScanResult> {
        let started_at = chrono::Utc::now();
        let stage_count = self.modules.len() + 2;
        progress(ScanProgress { stage: "machine".into(), stage_index: 0, stage_count, message: "Reading computer information".into() });
        let machine = platform.machine_info()?;
        let mut warnings = Vec::new();
        progress(ScanProgress { stage: "profiles".into(), stage_index: 1, stage_count, message: "Enumerating local user profiles".into() });
        let mut users = platform.list_profiles()?;
        users.sort_by(|a, b| b.is_current_user.cmp(&a.is_current_user).then(a.account_name.cmp(&b.account_name)));
        let processes = platform.running_processes();
        let ctx = DiscoveryContext {
            platform,
            rules: ExclusionRules::new(platform.system_roots()),
            cancel: cancel.clone(),
            processes: processes.clone(),
            measure_sizes,
        };
        let mut acc = ScanAccumulator::default();
        for (i, m) in self.modules.iter().enumerate() {
            cancel.check()?;
            progress(ScanProgress { stage: m.id().into(), stage_index: i + 2, stage_count, message: m.stage().into() });
            if let Err(e) = m.discover(&ctx, &users, &mut acc) {
                if matches!(e, crate::error::AppError::Canceled) {
                    return Err(e);
                }
                // One failing module never aborts the scan.
                warnings.push(Warning::warn(WarningCode::AdapterUnavailable, format!("{} could not be completed: {e}", m.stage())));
            }
        }
        // Approximate profile sizes from what was measured (labelled approximate in UI).
        for u in &mut users {
            let sum: u64 = acc.items.iter().filter(|i| i.owner.as_ref().is_some_and(|o| o.sid == u.sid)).filter_map(|i| i.estimated_size).sum();
            if sum > 0 {
                u.size_bytes = Some(sum);
            }
        }
        warnings.extend(acc.warnings);
        Ok(ScanResult {
            scan_id: uuid::Uuid::new_v4().to_string(),
            started_at,
            finished_at: chrono::Utc::now(),
            elevated: platform.is_elevated(),
            machine,
            users,
            items: acc.items,
            printers: acc.printers,
            mapped_drives: acc.mapped_drives,
            applications: acc.applications,
            browser_profiles: acc.browser_profiles,
            running_processes: processes,
            warnings,
            platform: platform.name(),
        })
    }
}

/// Build a custom-folder item chosen by the technician. Rejected when the
/// folder is a system location, contains one, or is a sensitive store.
pub fn custom_folder_item(ctx: &DiscoveryContext, path: &Path, owner: Option<&UserProfile>) -> AppResult<DiscoveryItem> {
    use crate::security::safe_path;
    let canonical = safe_path::canonicalize(path)?;
    if !canonical.is_dir() {
        return Err(crate::error::AppError::InvalidRequest(format!("{} is not a folder", canonical.display())));
    }
    let meta = std::fs::symlink_metadata(path).map_err(|e| crate::error::AppError::io(path, e))?;
    if meta.file_type().is_symlink() {
        return Err(crate::error::AppError::UnsafePath("links and junctions cannot be selected as capture roots".into()));
    }
    if let Some(reason) = ctx.rules.check_root(&canonical) {
        return Err(crate::error::AppError::UnsafePath(format!("{}: {}", canonical.display(), reason.describe())));
    }
    let name = canonical.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| canonical.display().to_string());
    let mut item = base_item(
        item_id("custom", owner, &canonical.display().to_string()),
        Category::UsersFiles,
        format!("Custom folder: {name}"),
        "Folder added by the technician. Copied recursively with standard exclusions.",
        path_source(&canonical),
        owner,
        RestoreKind::CustomFolder,
        ItemPayload::Folder { root: canonical.clone() },
    );
    item.includes = vec!["All files and subfolders".into()];
    item.excludes = vec!["Temporary files, recycle bins, links/junctions, protected credential and key files".into()];
    item.restore_notes =
        vec!["Restored to the same path inside the mapped user's profile, or to \"Migrated Files\" in that profile when it was outside the profile.".into()];
    measure(&mut item, ctx)?;
    item.selected_by_default = true;
    Ok(item)
}
