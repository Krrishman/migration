//! Opt-in application-settings plug-in framework.
//!
//! Each plug-in declares exactly what it reads and what it never reads. There
//! is no generic registry scraping: a plug-in can only name files/folders
//! relative to well-known per-user roots, and every plug-in item is opt-in.

use super::*;
use crate::error::AppResult;
use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginRoot {
    RoamingAppData,
    LocalAppData,
}

#[derive(Debug, Clone, Serialize)]
pub struct PluginPath {
    pub root: PluginRoot,
    /// Forward-slash path relative to the root.
    pub rel: &'static str,
    /// Only these files (relative to `rel`); empty = whole folder (with exclusions).
    pub files: &'static [&'static str],
    /// Only files with these extensions directly in the folder; empty = no filter.
    pub extensions: &'static [&'static str],
}

/// Declarative description of a settings plug-in.
#[derive(Debug, Clone, Serialize)]
pub struct SettingsPlugin {
    pub app_id: &'static str,
    pub display_name: &'static str,
    pub supported_versions: &'static str,
    pub discovery_paths: Vec<PluginPath>,
    pub discovery_registry_keys: &'static [&'static str],
    pub included_settings: &'static [&'static str],
    pub excluded_sensitive_data: &'static [&'static str],
    pub compatibility_notes: &'static [&'static str],
    /// Capture/restore strategy, for documentation and the details pane.
    pub capture: &'static str,
    pub restore: &'static str,
    /// Built-in plug-ins implemented by dedicated modules are listed for
    /// transparency only; they are discovered elsewhere.
    pub implemented_by_module: Option<&'static str>,
}

pub fn registry() -> Vec<SettingsPlugin> {
    vec![
        SettingsPlugin {
            app_id: "office-templates",
            display_name: "Microsoft Office templates & custom dictionary",
            supported_versions: "Office 2013–2021, Microsoft 365 (desktop)",
            discovery_paths: vec![
                PluginPath { root: PluginRoot::RoamingAppData, rel: "Microsoft/Templates", files: &[], extensions: &["dotx", "dotm", "potx", "potm", "xltx", "xltm", "thmx"] },
                PluginPath { root: PluginRoot::RoamingAppData, rel: "Microsoft/UProof", files: &["CUSTOM.DIC"], extensions: &[] },
            ],
            discovery_registry_keys: &[],
            included_settings: &["Normal.dotm and other document templates", "Custom spelling dictionary (CUSTOM.DIC)"],
            excluded_sensitive_data: &["Office licensing/activation", "Account sign-in and identity cache", "Recent-file lists"],
            compatibility_notes: &["Close Word before restoring Normal.dotm.", "Templates with macros (.dotm) may be blocked by the destination's macro policy."],
            capture: "File copy of the listed files",
            restore: "File copy into the mapped user's same folders, honoring the collision policy",
            implemented_by_module: None,
        },
        SettingsPlugin {
            app_id: "browser-chrome",
            display_name: "Google Chrome profile data",
            supported_versions: "Chrome 100+",
            discovery_paths: vec![],
            discovery_registry_keys: &[],
            included_settings: &["Bookmarks", "History", "Preferences (partial)", "Extensions (partial)"],
            excluded_sensitive_data: &["Passwords", "Cookies", "Tokens", "Autofill/payment data", "Local State key container"],
            compatibility_notes: &["Browser must be closed during capture and restore."],
            capture: "Allow-listed file copy",
            restore: "Bookmarks HTML export plus optional file restore into a profile",
            implemented_by_module: Some("browsers"),
        },
        SettingsPlugin {
            app_id: "browser-edge",
            display_name: "Microsoft Edge profile data",
            supported_versions: "Edge (Chromium) 100+",
            discovery_paths: vec![],
            discovery_registry_keys: &[],
            included_settings: &["Favorites", "History", "Preferences (partial)", "Extensions (partial)"],
            excluded_sensitive_data: &["Passwords", "Cookies", "Tokens", "Autofill/payment data", "Local State key container"],
            compatibility_notes: &["Browser must be closed during capture and restore."],
            capture: "Allow-listed file copy",
            restore: "Favorites HTML export plus optional file restore into a profile",
            implemented_by_module: Some("browsers"),
        },
        SettingsPlugin {
            app_id: "browser-firefox",
            display_name: "Mozilla Firefox profile data",
            supported_versions: "Firefox 100+ / ESR",
            discovery_paths: vec![],
            discovery_registry_keys: &[],
            included_settings: &["Bookmarks and history", "Preferences", "Search engines", "Containers"],
            excluded_sensitive_data: &["logins.json / key4.db", "cookies.sqlite", "cert9.db", "Firefox Account sign-in"],
            compatibility_notes: &["Destination Firefox should be the same or newer version."],
            capture: "Allow-listed file copy",
            restore: "Bookmarks HTML export plus optional file restore into a profile",
            implemented_by_module: Some("browsers"),
        },
        SettingsPlugin {
            app_id: "outlook-signatures",
            display_name: "Outlook signatures, templates and stationery",
            supported_versions: "Outlook 2013–2021, Microsoft 365 classic Outlook",
            discovery_paths: vec![],
            discovery_registry_keys: &[r"HKCU\Software\Microsoft\Office\16.0\Outlook\Profiles (names only)"],
            included_settings: &["Signatures", "Templates (.oft)", "Stationery", "PST files (opt-in)"],
            excluded_sensitive_data: &["Account passwords and tokens", "OST caches"],
            compatibility_notes: &["New Outlook stores signatures in the cloud."],
            capture: "File copy",
            restore: "File copy after user mapping confirmation",
            implemented_by_module: Some("outlook"),
        },
        SettingsPlugin {
            app_id: "windows-personalization",
            display_name: "Windows personalization",
            supported_versions: "Windows 10 1809+ / Windows 11",
            discovery_paths: vec![],
            discovery_registry_keys: &[r"HKCU\Control Panel\Desktop (WallPaper, read; written only on confirmed restore)"],
            included_settings: &["Wallpaper image", "Saved .theme files", "Slideshow folders", "Sticky Notes (opt-in)"],
            excluded_sensitive_data: &["Lock-screen and account pictures"],
            compatibility_notes: &["Start/taskbar layouts are not restored."],
            capture: "File copy",
            restore: "File copy; wallpaper applied for the signed-in user after confirmation",
            implemented_by_module: Some("personalization"),
        },
    ]
}

pub struct AppSettingsModule {
    pub plugins: Vec<SettingsPlugin>,
}

impl Default for AppSettingsModule {
    fn default() -> Self {
        Self { plugins: registry() }
    }
}

impl DiscoveryModule for AppSettingsModule {
    fn id(&self) -> &'static str {
        "plugins"
    }
    fn stage(&self) -> &'static str {
        "Checking application settings plug-ins"
    }
    fn discover(&self, ctx: &DiscoveryContext, users: &[UserProfile], acc: &mut ScanAccumulator) -> AppResult<()> {
        for user in eligible_users(users) {
            for plugin in self.plugins.iter().filter(|p| p.implemented_by_module.is_none()) {
                for (i, pp) in plugin.discovery_paths.iter().enumerate() {
                    let base = match pp.root {
                        PluginRoot::RoamingAppData => ctx.platform.roaming_app_data(user),
                        PluginRoot::LocalAppData => ctx.platform.local_app_data(user),
                    };
                    let mut root = base;
                    for part in pp.rel.split('/') {
                        root.push(part);
                    }
                    if !root.is_dir() {
                        continue;
                    }
                    let payload = if !pp.files.is_empty() {
                        ItemPayload::AllowList { root: root.clone(), files: pp.files.iter().map(|s| s.to_string()).collect(), dirs: vec![] }
                    } else if !pp.extensions.is_empty() {
                        ItemPayload::FilesByExtension { root: root.clone(), extensions: pp.extensions.iter().map(|s| s.to_string()).collect() }
                    } else {
                        ItemPayload::Folder { root: root.clone() }
                    };
                    let mut item = base_item(
                        item_id(&format!("plugin-{}", plugin.app_id), Some(user), &i.to_string()),
                        Category::ApplicationSettings,
                        format!("{} ({})", plugin.display_name, pp.rel.rsplit('/').next().unwrap_or(pp.rel)),
                        format!("Settings plug-in \"{}\" — supported versions: {}", plugin.app_id, plugin.supported_versions),
                        path_source(&root),
                        Some(user),
                        RestoreKind::OfficeTemplates,
                        payload,
                    );
                    item.support = SupportLevel::Partial;
                    item.opt_in_only = true;
                    item.includes = plugin.included_settings.iter().map(|s| s.to_string()).collect();
                    item.excludes = plugin.excluded_sensitive_data.iter().map(|s| s.to_string()).collect();
                    item.restore_notes = plugin.compatibility_notes.iter().map(|s| s.to_string()).collect();
                    measure(&mut item, ctx)?;
                    if item.item_count != Some(0) {
                        acc.items.push(item);
                    }
                }
            }
        }
        Ok(())
    }
}
