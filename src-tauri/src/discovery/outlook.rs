//! Outlook & e-mail: signatures, templates, stationery, PST files (opt-in),
//! OST files (shown, never captured) and profile names (inventory only).
//! No account settings, passwords or tokens are read.

use super::*;
use crate::error::AppResult;

pub struct OutlookModule;

pub const OUTLOOK_PROCESSES: &[&str] = &["outlook.exe"];

impl DiscoveryModule for OutlookModule {
    fn id(&self) -> &'static str {
        "outlook"
    }
    fn stage(&self) -> &'static str {
        "Discovering Outlook data"
    }
    fn discover(&self, ctx: &DiscoveryContext, users: &[UserProfile], acc: &mut ScanAccumulator) -> AppResult<()> {
        let outlook_running = ctx.process_running(OUTLOOK_PROCESSES);
        for user in eligible_users(users) {
            let roaming = ctx.platform.roaming_app_data(user).join("Microsoft");
            let local = ctx.platform.local_app_data(user).join("Microsoft").join("Outlook");

            let sig = roaming.join("Signatures");
            if sig.is_dir() {
                let mut item = base_item(
                    item_id("outlook", Some(user), "signatures"),
                    Category::OutlookEmail,
                    "Outlook signatures",
                    "E-mail signatures (HTML, RTF, text and their image folders).",
                    path_source(&sig),
                    Some(user),
                    RestoreKind::OutlookSignatures,
                    ItemPayload::Folder { root: sig },
                );
                item.includes = vec!["All signature files and *_files resource folders".into()];
                item.excludes = vec!["Signature assignments per account (stored with the mail profile)".into()];
                item.restore_notes = vec!["Copied to %APPDATA%\\Microsoft\\Signatures for the mapped user. Re-select default signatures in Outlook > Options > Mail > Signatures.".into(),
                    "New Outlook / Outlook on the web store signatures in the cloud and do not use these files.".into()];
                measure(&mut item, ctx)?;
                apply_default_selection(&mut item, Some(user));
                acc.items.push(item);
            }

            let templates = roaming.join("Templates");
            if templates.is_dir() {
                let mut item = base_item(
                    item_id("outlook", Some(user), "templates"),
                    Category::OutlookEmail,
                    "Outlook templates (.oft)",
                    "Outlook item templates saved by the user.",
                    path_source(&templates),
                    Some(user),
                    RestoreKind::OutlookTemplates,
                    ItemPayload::FilesByExtension { root: templates, extensions: vec!["oft".into()] },
                );
                item.includes = vec!["*.oft files in the user's Templates folder".into()];
                item.excludes = vec!["Office document templates (see Application Settings)".into()];
                item.restore_notes = vec!["Copied as files to the mapped user's Templates folder.".into()];
                measure(&mut item, ctx)?;
                if item.item_count != Some(0) {
                    apply_default_selection(&mut item, Some(user));
                    acc.items.push(item);
                }
            }

            let stationery = roaming.join("Stationery");
            if stationery.is_dir() {
                let mut item = base_item(
                    item_id("outlook", Some(user), "stationery"),
                    Category::OutlookEmail,
                    "Outlook stationery",
                    "Custom stationery and themes for e-mail.",
                    path_source(&stationery),
                    Some(user),
                    RestoreKind::OutlookStationery,
                    ItemPayload::Folder { root: stationery },
                );
                item.restore_notes = vec!["Copied as files to the mapped user's Stationery folder.".into()];
                measure(&mut item, ctx)?;
                apply_default_selection(&mut item, Some(user));
                acc.items.push(item);
            }

            // PST and OST discovery in the standard locations only.
            let documents = ctx.platform.known_folder(user, KnownFolder::Documents);
            let mut data_files: Vec<PathBuf> = Vec::new();
            for dir in [documents.join("Outlook Files"), local.clone()] {
                if let Ok(rd) = std::fs::read_dir(&dir) {
                    for e in rd.flatten() {
                        let p = e.path();
                        if p.is_file() && has_extension(&p, &["pst".into(), "ost".into()]) {
                            data_files.push(p);
                        }
                    }
                }
            }
            data_files.sort();
            for f in data_files {
                let name = f.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                let is_ost = has_extension(&f, &["ost".into()]);
                if is_ost {
                    let mut item = base_item(
                        item_id("ost", Some(user), &name),
                        Category::OutlookEmail,
                        format!("{name} (offline cache)"),
                        "Outlook offline cache (OST). Server-synced; Outlook rebuilds it on the destination.",
                        path_source(&f),
                        Some(user),
                        RestoreKind::Inventory { name: "outlook".into() },
                        ItemPayload::None,
                    );
                    item.support = SupportLevel::Unsupported;
                    item.estimated_size = std::fs::metadata(&f).ok().map(|m| m.len());
                    item.item_count = Some(1);
                    item.access = AccessState::Accessible;
                    item.warnings.push(Warning::info(WarningCode::ServerSyncedCache, "OST files are caches of server mailboxes and cannot be opened on another PC or profile. They are excluded; mail re-downloads after sign-in."));
                    item.restore_notes = vec!["Sign in to Outlook on the destination; the mailbox re-synchronizes from the server.".into()];
                    acc.items.push(item);
                    continue;
                }
                let mut item = base_item(
                    item_id("pst", Some(user), &name),
                    Category::OutlookEmail,
                    format!("{name} (Outlook data file)"),
                    "Personal Storage Table (PST) data file.",
                    path_source(&f),
                    Some(user),
                    RestoreKind::PstFile,
                    ItemPayload::Files { files: vec![f.clone()] },
                );
                item.opt_in_only = true;
                item.includes = vec!["The PST file as-is".into()];
                item.excludes = vec!["Account passwords and server settings".into()];
                item.restore_notes = vec![
                    "Copied to Documents\\Outlook Files for the mapped user. Open it in Outlook via File > Open & Export > Open Outlook Data File.".into(),
                    "Password-protected PSTs keep their password; Migration Assistant cannot and does not remove it.".into(),
                ];
                measure(&mut item, ctx)?;
                if item.estimated_size.unwrap_or(0) > 10 * 1024 * 1024 * 1024 {
                    item.warnings.push(Warning::warn(WarningCode::LargeItem, "Large PST file. Copying may take a long time; consider importing it into the mailbox instead."));
                }
                if !outlook_running.is_empty() {
                    item.access = AccessState::Locked;
                    item.warnings.push(Warning::warn(WarningCode::ApplicationRunning, "Outlook is running and keeps PST files open. Close Outlook before capture."));
                }
                acc.items.push(item);
            }

            let profiles = ctx.platform.outlook_profiles(user);
            if !profiles.is_empty() {
                let mut item = base_item(
                    item_id("outlook", Some(user), "profiles"),
                    Category::OutlookEmail,
                    "Outlook mail profile names",
                    format!("Profiles: {}", profiles.join(", ")),
                    SourceRef::Config { reference: r"HKCU\Software\Microsoft\Office\16.0\Outlook\Profiles (names only)".into() },
                    Some(user),
                    RestoreKind::Inventory { name: "outlook".into() },
                    ItemPayload::Inventory { name: "outlook".into() },
                );
                item.support = SupportLevel::InventoryOnly;
                item.item_count = Some(profiles.len() as u64);
                item.access = AccessState::Accessible;
                item.includes = vec!["Mail profile names, signature/template/PST locations, recommended restore procedure".into()];
                item.excludes = vec!["Account server settings, passwords, tokens".into()];
                item.restore_notes = vec!["Recreate the mail profile by signing in to Outlook on the destination (Autodiscover configures most accounts).".into()];
                apply_default_selection(&mut item, Some(user));
                acc.items.push(item);
            }
        }
        Ok(())
    }
}
