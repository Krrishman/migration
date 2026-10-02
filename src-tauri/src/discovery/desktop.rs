//! Desktop & shortcuts: user Desktop, Public Desktop, Start menu shortcuts,
//! taskbar pins, Quick Access and (opt-in, privacy-sensitive) recent items.

use super::*;
use crate::error::AppResult;

pub struct DesktopModule;

impl DiscoveryModule for DesktopModule {
    fn id(&self) -> &'static str {
        "desktop"
    }
    fn stage(&self) -> &'static str {
        "Scanning desktops and shortcuts"
    }
    fn discover(&self, ctx: &DiscoveryContext, users: &[UserProfile], acc: &mut ScanAccumulator) -> AppResult<()> {
        for user in eligible_users(users) {
            let desktop = ctx.platform.known_folder(user, KnownFolder::Desktop);
            if desktop.exists() {
                let mut item = base_item(
                    item_id("folder", Some(user), "Desktop"),
                    Category::DesktopShortcuts,
                    "Desktop",
                    format!("{}'s desktop files and shortcuts", user.account_name),
                    path_source(&desktop),
                    Some(user),
                    RestoreKind::KnownFolder { folder: KnownFolder::Desktop },
                    ItemPayload::Folder { root: desktop.clone() },
                );
                item.includes = vec!["Files, folders and shortcuts on the user's desktop".into()];
                item.excludes = vec!["Temporary files, links/junctions, online-only cloud files".into()];
                item.restore_notes = vec!["Restored into the destination user's Desktop. Shortcuts to applications that are not installed will not work until the app is reinstalled.".into()];
                measure(&mut item, ctx)?;
                apply_default_selection(&mut item, Some(user));
                acc.items.push(item);
            }

            let roaming = ctx.platform.roaming_app_data(user);
            let start_menu = roaming.join("Microsoft").join("Windows").join("Start Menu").join("Programs");
            if start_menu.is_dir() {
                let mut item = base_item(
                    item_id("startmenu", Some(user), "programs"),
                    Category::DesktopShortcuts,
                    "Start menu shortcuts (user)",
                    "Per-user Start menu program shortcuts.",
                    path_source(&start_menu),
                    Some(user),
                    RestoreKind::StartMenuShortcuts,
                    ItemPayload::Folder { root: start_menu },
                );
                item.support = SupportLevel::Partial;
                item.includes = vec!["Shortcut files (.lnk/.url) in the user's Start menu Programs folder".into()];
                item.excludes = vec!["Start menu layout/pins (not portable across Windows versions)".into()];
                item.restore_notes = vec!["Shortcuts are copied as files. The Windows 11 Start layout and pinned tiles are not restored.".into()];
                item.warnings.push(Warning::info(
                    WarningCode::VersionDependent,
                    "Start menu layout differs between Windows 10 and 11; only shortcut files are migrated.",
                ));
                measure(&mut item, ctx)?;
                apply_default_selection(&mut item, Some(user));
                acc.items.push(item);
            }

            let taskbar = roaming.join("Microsoft").join("Internet Explorer").join("Quick Launch").join("User Pinned").join("TaskBar");
            if taskbar.is_dir() {
                let mut item = base_item(
                    item_id("taskbar", Some(user), "pins"),
                    Category::DesktopShortcuts,
                    "Taskbar pinned shortcuts",
                    "Shortcut files behind pinned taskbar items.",
                    path_source(&taskbar),
                    Some(user),
                    RestoreKind::TaskbarPins,
                    ItemPayload::Folder { root: taskbar },
                );
                item.support = SupportLevel::Partial;
                item.opt_in_only = true;
                item.includes = vec!["Pinned-item shortcut files".into()];
                item.excludes = vec!["Taskbar layout and order".into()];
                item.restore_notes =
                    vec!["Copied to \"Migrated Files\\Taskbar shortcuts\" for the user to re-pin. Windows does not support programmatic pinning.".into()];
                item.warnings.push(Warning::info(
                    WarningCode::VersionDependent,
                    "Taskbar pins cannot be re-applied automatically; the shortcuts are provided for manual pinning.",
                ));
                measure(&mut item, ctx)?;
                acc.items.push(item);
            }

            let recent = roaming.join("Microsoft").join("Windows").join("Recent");
            let quick_access = recent.join("AutomaticDestinations").join("f01b4d95cf55d32a.automaticDestinations-ms");
            if quick_access.is_file() {
                let mut item = base_item(
                    item_id("quickaccess", Some(user), "pins"),
                    Category::DesktopShortcuts,
                    "Quick Access pinned folders (best effort)",
                    "Explorer Quick Access jump-list file.",
                    path_source(&quick_access),
                    Some(user),
                    RestoreKind::QuickAccess,
                    ItemPayload::Files { files: vec![quick_access] },
                );
                item.support = SupportLevel::Partial;
                item.opt_in_only = true;
                item.includes = vec!["Quick Access pin list file".into()];
                item.excludes = vec!["Other jump lists".into()];
                item.restore_notes = vec!["Best effort: restored only if the destination has no Quick Access customizations; pins pointing to paths that do not exist on the destination are ignored by Explorer.".into()];
                item.warnings.push(Warning::warn(
                    WarningCode::VersionDependent,
                    "Format is undocumented and may change between Windows builds. Not guaranteed to restore.",
                ));
                measure(&mut item, ctx)?;
                acc.items.push(item);
            }

            if recent.is_dir() {
                let mut item = base_item(
                    item_id("recent", Some(user), "items"),
                    Category::DesktopShortcuts,
                    "Recent items (privacy-sensitive)",
                    "Shortcuts to recently opened files. Reveals user activity.",
                    path_source(&recent),
                    Some(user),
                    RestoreKind::RecentItems,
                    ItemPayload::FilesByExtension { root: recent, extensions: vec!["lnk".into()] },
                );
                item.support = SupportLevel::Partial;
                item.sensitive = true;
                item.opt_in_only = true;
                item.includes = vec!["Recent-item shortcut files (.lnk)".into()];
                item.excludes = vec!["Jump list databases".into()];
                item.restore_notes = vec!["Copied to \"Migrated Files\\Recent items\" for reference only.".into()];
                item.warnings
                    .push(Warning::warn(WarningCode::PrivacySensitive, "Recent items reveal what the user opened. Only capture with the user's knowledge."));
                measure(&mut item, ctx)?;
                acc.items.push(item);
            }
        }

        if let Some(public) = ctx.platform.public_desktop().filter(|p| p.is_dir()) {
            let mut item = base_item(
                item_id("publicdesktop", None, "public"),
                Category::DesktopShortcuts,
                "Public Desktop",
                "Items shown on every user's desktop.",
                path_source(&public),
                None,
                RestoreKind::PublicDesktop,
                ItemPayload::Folder { root: public },
            );
            item.includes = vec!["Files and shortcuts on the Public Desktop".into()];
            item.excludes = vec!["desktop.ini and links/junctions".into()];
            item.restore_notes = vec!["Writing to the Public Desktop on the destination requires administrator rights.".into()];
            measure(&mut item, ctx)?;
            item.requires_admin = false;
            apply_default_selection(&mut item, None);
            acc.items.push(item);
        }
        Ok(())
    }
}
