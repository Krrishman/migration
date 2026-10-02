//! Personalization: wallpaper, slideshow folders, theme files and Sticky Notes.

use super::*;
use crate::error::AppResult;
use crate::security::safe_path::is_within;

pub struct PersonalizationModule;

pub const STICKY_NOTES_PACKAGE: &str = "Microsoft.MicrosoftStickyNotes_8wekyb3d8bbwe";
pub const STICKY_NOTES_PROCESSES: &[&str] = &["Microsoft.Notes.exe", "StikyNot.exe"];

/// Extract `Wallpaper=` and `[Slideshow] ImagesRootPath=` from a .theme file.
pub fn parse_theme_file(text: &str) -> (Option<String>, Option<String>) {
    let mut section = String::new();
    let mut wallpaper = None;
    let mut slideshow = None;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            section = line.to_ascii_lowercase();
        } else if let Some((k, v)) = line.split_once('=') {
            let k = k.trim().to_ascii_lowercase();
            let v = v.trim().to_string();
            if section == "[control panel\\desktop]" && k == "wallpaper" && !v.is_empty() {
                wallpaper = Some(v);
            } else if section == "[slideshow]" && k == "imagesrootpath" && !v.is_empty() {
                slideshow = Some(v);
            }
        }
    }
    (wallpaper, slideshow)
}

impl DiscoveryModule for PersonalizationModule {
    fn id(&self) -> &'static str {
        "personalization"
    }
    fn stage(&self) -> &'static str {
        "Reading personalization"
    }
    fn discover(&self, ctx: &DiscoveryContext, users: &[UserProfile], acc: &mut ScanAccumulator) -> AppResult<()> {
        for user in eligible_users(users) {
            if let Some(wp) = ctx.platform.wallpaper_path(user).filter(|p| p.is_file()) {
                let mut item = base_item(
                    item_id("wallpaper", Some(user), "current"),
                    Category::Personalization,
                    "Desktop wallpaper",
                    format!("Current wallpaper image: {}", wp.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()),
                    path_source(&wp),
                    Some(user),
                    RestoreKind::Wallpaper,
                    ItemPayload::Files { files: vec![wp] },
                );
                item.support = SupportLevel::Partial;
                item.includes = vec!["The wallpaper image file".into()];
                item.excludes = vec!["Lock screen and Spotlight images".into()];
                item.restore_notes = vec![
                    "Image copied to Pictures\\Migrated Wallpaper for the mapped user.".into(),
                    "Applying it as wallpaper writes HKCU\\Control Panel\\Desktop\\WallPaper for the signed-in user only, after confirmation.".into(),
                ];
                measure(&mut item, ctx)?;
                apply_default_selection(&mut item, Some(user));
                acc.items.push(item);
            }

            let themes = ctx.platform.local_app_data(user).join("Microsoft").join("Windows").join("Themes");
            if themes.is_dir() {
                let mut item = base_item(
                    item_id("themes", Some(user), "custom"),
                    Category::Personalization,
                    "Saved themes",
                    "Custom .theme files and theme packs saved by the user.",
                    path_source(&themes),
                    Some(user),
                    RestoreKind::Themes,
                    ItemPayload::Folder { root: themes.clone() },
                );
                item.support = SupportLevel::Partial;
                item.includes = vec!["*.theme, *.deskthemepack and their wallpaper folders".into()];
                item.excludes = vec!["High-contrast and system themes".into()];
                item.restore_notes = vec!["Theme files are copied; double-click a .theme file on the destination to apply it.".into()];
                item.warnings.push(Warning::info(WarningCode::VersionDependent, "Theme rendering differs between Windows 10 and 11."));
                measure(&mut item, ctx)?;
                apply_default_selection(&mut item, Some(user));

                // Slideshow folders referenced by .theme files, when readable and not already covered.
                if let Ok(rd) = std::fs::read_dir(&themes) {
                    for e in rd.flatten() {
                        let p = e.path();
                        if !has_extension(&p, &["theme".into()]) {
                            continue;
                        }
                        let Ok(text) = std::fs::read_to_string(&p) else { continue };
                        if let (_, Some(folder)) = parse_theme_file(&text) {
                            let folder = PathBuf::from(crate::windows::expand_env(&folder));
                            let covered = KnownFolder::ALL.iter().any(|kf| is_within(&ctx.platform.known_folder(user, *kf), &folder));
                            if folder.is_dir() && !covered && ctx.rules.check_root(&folder).is_none() {
                                let mut s = base_item(
                                    item_id("slideshow", Some(user), &folder.display().to_string()),
                                    Category::Personalization,
                                    "Wallpaper slideshow folder",
                                    "Folder used by the wallpaper slideshow.",
                                    path_source(&folder),
                                    Some(user),
                                    RestoreKind::CustomFolder,
                                    ItemPayload::Folder { root: folder.clone() },
                                );
                                s.support = SupportLevel::Partial;
                                s.restore_notes = vec!["Images are restored; re-select the folder in Settings > Personalization > Background.".into()];
                                measure(&mut s, ctx)?;
                                acc.items.push(s);
                            } else if covered {
                                item.restore_notes.push(format!("Slideshow folder {} is part of a user folder selected above.", folder.display()));
                            }
                        }
                    }
                }
                acc.items.push(item);
            }

            let sticky = ctx.platform.local_app_data(user).join("Packages").join(STICKY_NOTES_PACKAGE).join("LocalState");
            if sticky.join("plum.sqlite").is_file() {
                let mut item = base_item(
                    item_id("stickynotes", Some(user), "plum"),
                    Category::Personalization,
                    "Sticky Notes (Windows 10/11 app)",
                    "Local Sticky Notes database for the Microsoft Store app.",
                    path_source(&sticky),
                    Some(user),
                    RestoreKind::StickyNotes,
                    ItemPayload::AllowList { root: sticky, files: vec!["plum.sqlite".into(), "plum.sqlite-wal".into(), "plum.sqlite-shm".into()], dirs: vec![] },
                );
                item.support = SupportLevel::Partial;
                item.opt_in_only = true;
                item.includes = vec!["plum.sqlite (+ WAL/SHM journal files)".into()];
                item.excludes = vec!["Microsoft account sync state".into()];
                item.restore_notes = vec![
                    "Restored only when Sticky Notes is closed and the destination has no existing notes database (or per your collision policy).".into(),
                    "If the user signs in to Sticky Notes with a Microsoft account, notes sync from the cloud instead.".into(),
                ];
                item.warnings.push(Warning::info(WarningCode::VersionDependent, "Database format depends on the Sticky Notes app version; the destination should run the same or a newer version."));
                let running = ctx.process_running(STICKY_NOTES_PROCESSES);
                if !running.is_empty() {
                    item.warnings.push(Warning::warn(WarningCode::ApplicationRunning, "Sticky Notes is running. Close it before capture."));
                }
                measure(&mut item, ctx)?;
                acc.items.push(item);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_theme_files() {
        let t = "[Theme]\nDisplayName=Custom\n[Control Panel\\Desktop]\nWallpaper=%USERPROFILE%\\Pictures\\a.jpg\n[Slideshow]\nInterval=60000\nImagesRootPath=D:\\Photos\\Wallpapers\n";
        let (w, s) = parse_theme_file(t);
        assert_eq!(w.as_deref(), Some("%USERPROFILE%\\Pictures\\a.jpg"));
        assert_eq!(s.as_deref(), Some("D:\\Photos\\Wallpapers"));
        assert_eq!(parse_theme_file("[Theme]\n"), (None, None));
    }
}
