//! User folders: known folders (except Desktop, which lives under
//! "Desktop & Shortcuts") and locally present OneDrive content.

use super::*;
use crate::error::AppResult;
use crate::security::safe_path::is_within;

pub struct UserFoldersModule;

impl DiscoveryModule for UserFoldersModule {
    fn id(&self) -> &'static str {
        "folders"
    }
    fn stage(&self) -> &'static str {
        "Measuring user folders"
    }
    fn discover(&self, ctx: &DiscoveryContext, users: &[UserProfile], acc: &mut ScanAccumulator) -> AppResult<()> {
        for user in eligible_users(users) {
            let onedrive = ctx.platform.onedrive_roots(user);
            let mut offered: Vec<PathBuf> = Vec::new();
            for folder in KnownFolder::ALL.into_iter().filter(|f| *f != KnownFolder::Desktop) {
                let path = ctx.platform.known_folder(user, folder);
                if !path.exists() {
                    continue;
                }
                let in_onedrive = onedrive.iter().any(|r| is_within(r, &path));
                let mut item = base_item(
                    item_id("folder", Some(user), folder.default_dir_name()),
                    Category::UsersFiles,
                    folder.default_dir_name(),
                    format!("{}'s {} folder{}", user.account_name, folder.default_dir_name(), if in_onedrive { " (redirected to OneDrive)" } else { "" }),
                    path_source(&path),
                    Some(user),
                    RestoreKind::KnownFolder { folder },
                    ItemPayload::Folder { root: path.clone() },
                );
                item.includes = vec!["All files and subfolders that are stored on this PC".into()];
                item.excludes = vec![
                    "Temporary files (~$*, *.tmp), recycle bins, links/junctions".into(),
                    "Online-only cloud placeholders (not downloaded)".into(),
                    "Private keys, password databases and credential files".into(),
                ];
                item.restore_notes = vec![format!("Restored into the destination user's {} folder (respecting its redirection).", folder.default_dir_name())];
                if in_onedrive {
                    item.support = SupportLevel::Partial;
                    item.warnings.push(Warning::info(
                        WarningCode::CloudPlaceholder,
                        "This folder is synced by OneDrive. Signing in to OneDrive on the destination is usually the better way to bring it back; only locally present files are captured.",
                    ));
                }
                if folder == KnownFolder::Downloads {
                    item.warnings.push(Warning::info(WarningCode::LargeItem, "Downloads often contains large installers that may not need migrating."));
                }
                measure(&mut item, ctx)?;
                apply_default_selection(&mut item, Some(user));
                offered.push(path);
                acc.items.push(item);
            }
            for root in onedrive {
                let name = root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "OneDrive".into());
                let mut item = base_item(
                    item_id("onedrive", Some(user), &name),
                    Category::UsersFiles,
                    format!("{name} (locally available files)"),
                    "OneDrive sync folder. Only files already on this PC are captured; online-only files are skipped and never downloaded.",
                    path_source(&root),
                    Some(user),
                    RestoreKind::OneDriveLocal,
                    ItemPayload::Folder { root: root.clone() },
                );
                item.support = SupportLevel::Partial;
                item.opt_in_only = true;
                item.includes = vec!["Locally present files in the OneDrive folder".into()];
                item.excludes = vec!["Online-only placeholders; OneDrive account and sync configuration".into()];
                item.restore_notes = vec![
                    "Prefer signing in to OneDrive on the destination PC. If restored, files land in a \"Migrated Files\" folder to avoid sync conflicts."
                        .into(),
                ];
                if offered.iter().any(|p| is_within(&root, p)) {
                    item.warnings.push(Warning::warn(
                        WarningCode::Other,
                        "Overlaps with redirected known folders listed above; selecting both captures those files twice.",
                    ));
                }
                measure(&mut item, ctx)?;
                acc.items.push(item);
            }
        }
        Ok(())
    }
}
