//! Installed application inventory with a small, local, curated catalog for
//! plain-language descriptions. No web lookups. Applications, program
//! folders and license keys are never copied.

use super::*;
use crate::error::AppResult;

/// (match prefix (lower-case), category, description, settings plug-in id)
const CATALOG: &[(&str, &str, &str, Option<&str>)] = &[
    ("google chrome", "Web browser", "Web browser by Google.", Some("browser-chrome")),
    ("microsoft edge", "Web browser", "Web browser included with Windows.", Some("browser-edge")),
    ("mozilla firefox", "Web browser", "Open-source web browser by Mozilla.", Some("browser-firefox")),
    ("brave", "Web browser", "Chromium-based web browser.", Some("browser-chromium")),
    ("vivaldi", "Web browser", "Chromium-based web browser.", Some("browser-chromium")),
    ("microsoft 365", "Productivity", "Microsoft Office apps (Word, Excel, PowerPoint, Outlook).", Some("office-templates")),
    ("microsoft office", "Productivity", "Microsoft Office desktop apps.", Some("office-templates")),
    ("libreoffice", "Productivity", "Open-source office suite.", None),
    ("adobe acrobat", "Documents/PDF", "PDF viewer and editor by Adobe.", None),
    ("foxit", "Documents/PDF", "PDF reader/editor.", None),
    ("7-zip", "Utilities", "File archiver.", None),
    ("winrar", "Utilities", "File archiver.", None),
    ("notepad++", "Developer tools", "Text and source-code editor.", None),
    ("microsoft visual studio code", "Developer tools", "Source-code editor by Microsoft.", None),
    ("git", "Developer tools", "Version-control system.", None),
    ("python", "Developer tools", "Python programming language runtime.", None),
    ("zoom", "Communication", "Video meetings client.", None),
    ("microsoft teams", "Communication", "Chat and meetings client by Microsoft.", None),
    ("slack", "Communication", "Team messaging client.", None),
    ("webex", "Communication", "Video meetings client by Cisco.", None),
    ("vlc media player", "Media", "Media player.", None),
    ("spotify", "Media", "Music streaming client.", None),
    ("microsoft onedrive", "Cloud storage", "OneDrive sync client (sign in on the destination).", None),
    ("dropbox", "Cloud storage", "Dropbox sync client (sign in on the destination).", None),
    ("google drive", "Cloud storage", "Google Drive sync client (sign in on the destination).", None),
    ("microsoft visual c++", "Runtime", "Runtime library required by other applications; usually installed automatically.", None),
    ("microsoft .net", "Runtime", "Runtime required by other applications.", None),
    ("java", "Runtime", "Java runtime.", None),
    ("teamviewer", "Remote support", "Remote support tool. Reinstall only if authorized by IT.", None),
    ("anydesk", "Remote support", "Remote support tool. Reinstall only if authorized by IT.", None),
    ("hp ", "Device software", "Printer/device software from HP.", None),
    ("intel", "Driver/Device software", "Hardware driver or utility; install the destination PC's own drivers instead.", None),
    ("realtek", "Driver/Device software", "Hardware driver; install the destination PC's own drivers instead.", None),
    ("nvidia", "Driver/Device software", "Graphics driver; install the destination PC's own drivers instead.", None),
];

/// Apply the curated catalog; unknown apps are labelled honestly.
pub fn annotate(mut app: InstalledApp) -> InstalledApp {
    let n = app.display_name.to_lowercase();
    match CATALOG.iter().find(|(prefix, ..)| n.starts_with(prefix)) {
        Some((_, cat, desc, plugin)) => {
            app.category = cat.to_string();
            app.description = Some(desc.to_string());
            app.settings_plugin = plugin.map(str::to_string);
        }
        None => {
            if app.category.is_empty() {
                app.category = "Other".into();
            }
            app.description = None;
        }
    }
    app
}

pub fn reinstall_checklist(apps: &[InstalledApp]) -> Vec<String> {
    apps.iter()
        .filter(|a| !matches!(a.category.as_str(), "Runtime" | "Driver/Device software"))
        .map(|a| {
            format!(
                "[ ] {}{}{}",
                a.display_name,
                a.version.as_deref().map(|v| format!(" {v}")).unwrap_or_default(),
                a.publisher.as_deref().map(|p| format!(" — {p}")).unwrap_or_default()
            )
        })
        .collect()
}

pub struct InstalledAppsModule;

impl DiscoveryModule for InstalledAppsModule {
    fn id(&self) -> &'static str {
        "apps"
    }
    fn stage(&self) -> &'static str {
        "Building application inventory"
    }
    fn discover(&self, ctx: &DiscoveryContext, _users: &[UserProfile], acc: &mut ScanAccumulator) -> AppResult<()> {
        let apps: Vec<InstalledApp> = match ctx.platform.installed_apps() {
            Ok(a) => a.into_iter().map(annotate).collect(),
            Err(e) => {
                acc.warnings.push(Warning::warn(WarningCode::AdapterUnavailable, format!("Installed application inventory unavailable: {e}")));
                return Ok(());
            }
        };
        let with_plugin = apps.iter().filter(|a| a.settings_plugin.is_some()).count();
        let mut item = base_item(
            item_id("inventory", None, "applications"),
            Category::InstalledApplications,
            format!("Installed applications inventory ({} apps)", apps.len()),
            "Name, version, publisher, install location/date and architecture for each installed application, plus a reinstall checklist.",
            SourceRef::Config { reference: r"HKLM/HKCU ...\CurrentVersion\Uninstall (64-bit, 32-bit and per-user)".into() },
            None,
            RestoreKind::Inventory { name: "applications".into() },
            ItemPayload::Inventory { name: "applications".into() },
        );
        item.support = SupportLevel::InventoryOnly;
        item.item_count = Some(apps.len() as u64);
        item.access = AccessState::Accessible;
        item.includes =
            vec!["Application inventory report".into(), "Recommended reinstall checklist".into(), format!("{with_plugin} app(s) have a settings plug-in")];
        item.excludes = vec!["Program files, installers, license/product keys and activation data are never copied".into()];
        item.restore_notes = vec!["Reinstall applications on the destination from their official sources, then restore settings plug-ins.".into()];
        item.warnings.push(Warning::info(
            WarningCode::Other,
            "Uninstall commands are recorded only in the technician report (marked advanced) and are redacted from the shareable summary.",
        ));
        apply_default_selection(&mut item, None);
        acc.items.push(item);
        acc.applications = apps;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(name: &str) -> InstalledApp {
        InstalledApp {
            display_name: name.into(),
            version: Some("1.0".into()),
            publisher: None,
            install_location: None,
            install_date: None,
            uninstall_command: None,
            architecture: "x64".into(),
            scope: "Machine".into(),
            category: String::new(),
            description: None,
            settings_plugin: None,
        }
    }

    #[test]
    fn annotates_known_and_unknown_apps() {
        let a = annotate(app("Google Chrome"));
        assert_eq!(a.category, "Web browser");
        assert_eq!(a.settings_plugin.as_deref(), Some("browser-chrome"));
        let u = annotate(app("Contoso Line-of-Business Tool"));
        assert_eq!(u.category, "Other");
        assert!(u.description.is_none());
    }

    #[test]
    fn checklist_skips_runtimes_and_drivers() {
        let apps: Vec<_> =
            ["Google Chrome", "Microsoft Visual C++ 2015-2022 Redistributable", "NVIDIA Graphics Driver"].iter().map(|n| annotate(app(n))).collect();
        let c = reinstall_checklist(&apps);
        assert_eq!(c.len(), 1);
        assert!(c[0].starts_with("[ ] Google Chrome"));
    }
}
