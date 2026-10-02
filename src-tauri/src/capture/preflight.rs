//! Capture preflight: destination checks, space, file-system limits, long
//! paths and open applications. Blocking errors stop the capture before any
//! bundle folder is created.

use crate::discovery::browsers::default_providers;
use crate::discovery::outlook::OUTLOOK_PROCESSES;
use crate::discovery::personalization::STICKY_NOTES_PROCESSES;
use crate::models::*;
use crate::platform::Platform;
use crate::security::exclusions::ExclusionRules;
use crate::security::safe_path::is_within;
use crate::util::format_bytes;
use std::path::Path;

/// Safety margin on top of the estimate (manifests, hash lists, reports, encryption overhead).
pub fn required_bytes(estimated: u64) -> u64 {
    estimated + estimated / 20 + 64 * 1024 * 1024
}

pub fn is_writable_dir(dir: &Path) -> bool {
    if !dir.is_dir() {
        return false;
    }
    let probe = dir.join(format!(".ma-write-test-{}", uuid::Uuid::new_v4()));
    match std::fs::write(&probe, b"probe") {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

pub fn run_preflight(platform: &dyn Platform, scan: &ScanResult, req: &CaptureRequest) -> PreflightReport {
    let dest = Path::new(&req.destination_root);
    let mut warnings = Vec::new();
    let mut blocking = Vec::new();

    let selected: Vec<&DiscoveryItem> = req.selected_item_ids.iter().filter_map(|id| scan.items.iter().find(|i| &i.id == id)).collect();
    if selected.len() != req.selected_item_ids.len() {
        blocking.push("Some selected items are no longer present in the scan. Rescan and select again.".into());
    }
    if selected.is_empty() {
        blocking.push("Nothing is selected.".into());
    }
    if let Some(bad) = selected.iter().find(|i| !i.support.is_capturable()) {
        blocking.push(format!("\"{}\" cannot be captured.", bad.display_name));
    }
    if req.encryption.enabled {
        match req.encryption.passphrase.as_deref() {
            Some(p) if p.chars().count() >= crate::security::encryption::MIN_PASSPHRASE_CHARS => {}
            _ => blocking.push(format!(
                "Encryption is enabled but no valid passphrase (at least {} characters) was provided.",
                crate::security::encryption::MIN_PASSPHRASE_CHARS
            )),
        }
    }

    let writable = is_writable_dir(dest);
    if !dest.is_dir() {
        blocking.push(format!("Destination folder does not exist: {}", dest.display()));
    } else if !writable {
        blocking.push(format!("Destination folder is not writable: {}", dest.display()));
    }
    let rules = ExclusionRules::new(platform.system_roots());
    if rules.system_roots.iter().any(|r| is_within(r, dest)) {
        blocking.push("The destination is inside a Windows or Program Files folder.".into());
    }
    // Never write the bundle inside a folder that is being captured (recursion).
    for i in &selected {
        if let ItemPayload::Folder { root } | ItemPayload::AllowList { root, .. } = &i.payload {
            if is_within(root, dest) {
                blocking.push(format!("The destination is inside \"{}\", which is selected for capture. Choose another destination.", i.display_name));
            }
        }
    }

    let estimated: u64 = selected.iter().filter_map(|i| i.estimated_size).sum();
    let unknown = selected.iter().filter(|i| i.estimated_size.is_none() && !matches!(i.payload, ItemPayload::Inventory { .. } | ItemPayload::None)).count();
    let dest_space = platform.disk_space(dest);
    let source_space = scan.users.iter().find(|u| u.is_current_user).and_then(|u| platform.disk_space(&u.profile_path));
    let need = required_bytes(estimated);
    let sufficient = match &dest_space {
        Some(d) => d.free_bytes >= need,
        None => true,
    };
    if let Some(d) = &dest_space {
        if !sufficient {
            blocking.push(format!(
                "Not enough free space on the destination: about {} needed, {} available.",
                format_bytes(need),
                format_bytes(d.free_bytes)
            ));
        } else if d.free_bytes < need + need / 5 {
            warnings.push(Warning::warn(WarningCode::LowDiskSpace, "Destination free space is tight; the capture may fail if files grow during copy."));
        }
    } else {
        warnings.push(Warning::warn(WarningCode::LowDiskSpace, "Destination free space could not be determined."));
    }
    if unknown > 0 {
        warnings.push(Warning::info(WarningCode::Other, format!("{unknown} selected item(s) have no size estimate; the space check may be low.")));
    }

    let fs = platform.file_system_of(dest);
    if fs.as_deref().is_some_and(|f| f.eq_ignore_ascii_case("fat32") || f.eq_ignore_ascii_case("vfat") || f.eq_ignore_ascii_case("fat")) {
        warnings.push(Warning::warn(WarningCode::LargeItem, "The destination uses FAT32: files of 4 GB or more cannot be stored and will be skipped. NTFS or exFAT is recommended."));
    }
    let long_paths = platform.long_paths_enabled();
    if long_paths == Some(false) {
        warnings.push(Warning::info(WarningCode::LongPath, "Long path support is disabled in Windows. Migration Assistant still copies long paths, but Explorer may not open them."));
    }

    // Running applications that hold relevant files open.
    let mut watch: Vec<(&str, String)> = Vec::new();
    for p in default_providers() {
        if selected.iter().any(|i| matches!(&i.restore_kind, RestoreKind::BrowserProfile { browser, .. } if *browser == p.kind())) {
            for n in p.process_names() {
                watch.push((n, p.display_name()));
            }
        }
    }
    if selected.iter().any(|i| i.category == Category::OutlookEmail) {
        watch.extend(OUTLOOK_PROCESSES.iter().map(|n| (*n, "Microsoft Outlook".to_string())));
    }
    if selected.iter().any(|i| i.restore_kind == RestoreKind::StickyNotes) {
        watch.extend(STICKY_NOTES_PROCESSES.iter().map(|n| (*n, "Sticky Notes".to_string())));
    }
    let processes = platform.running_processes();
    let mut running_apps: Vec<String> = watch
        .iter()
        .filter(|(n, _)| processes.iter().any(|p| p.name.eq_ignore_ascii_case(n)))
        .map(|(_, label)| label.clone())
        .collect();
    running_apps.sort();
    running_apps.dedup();
    for app in &running_apps {
        warnings.push(Warning::warn(
            WarningCode::ApplicationRunning,
            format!("{app} is running. Close it, then choose Retry detection. Locked files are skipped (and reported) if you continue."),
        ));
    }
    if selected.iter().any(|i| i.warnings.iter().any(|w| w.code == WarningCode::EfsEncrypted)) && !req.encryption.enabled {
        warnings.push(Warning::warn(WarningCode::EfsEncrypted, "EFS-encrypted files will be stored without EFS protection in the bundle. Enable bundle encryption to keep them protected at rest."));
    }

    PreflightReport {
        destination_root: dest.display().to_string(),
        destination_writable: writable,
        destination_free_bytes: dest_space.as_ref().map(|d| d.free_bytes),
        source_free_bytes: source_space.as_ref().map(|d| d.free_bytes),
        estimated_bytes: estimated,
        unknown_size_items: unknown,
        sufficient_space: sufficient,
        long_paths_enabled: long_paths,
        destination_file_system: fs,
        selected_count: selected.len(),
        running_apps,
        warnings,
        blocking_errors: blocking,
    }
}
