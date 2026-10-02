//! System inventory, printers and mapped network drives.

use super::*;
use crate::error::AppResult;

pub struct SystemInventoryModule;

impl DiscoveryModule for SystemInventoryModule {
    fn id(&self) -> &'static str {
        "system"
    }
    fn stage(&self) -> &'static str {
        "Collecting system inventory"
    }
    fn discover(&self, ctx: &DiscoveryContext, users: &[UserProfile], acc: &mut ScanAccumulator) -> AppResult<()> {
        let mut machine = base_item(
            item_id("inventory", None, "machine"),
            Category::SystemInventory,
            "Computer inventory",
            "Computer name, Windows edition/build, CPU/RAM summary, drives and network adapter names.",
            SourceRef::Config { reference: "Windows system information".into() },
            None,
            RestoreKind::Inventory { name: "machine".into() },
            ItemPayload::Inventory { name: "machine".into() },
        );
        machine.support = SupportLevel::InventoryOnly;
        machine.includes = vec!["Computer name, Windows version/build, architecture, time zone".into(), "CPU, memory, drives (size/free space), network adapter names".into(), "Domain/workgroup/Azure AD join indicator".into()];
        machine.excludes = vec!["IP configuration, Wi-Fi profiles and keys, product keys, any credentials".into()];
        machine.restore_notes = vec!["Reference only. Used in reports to compare source and destination PCs.".into()];
        machine.access = AccessState::Accessible;
        apply_default_selection(&mut machine, None);
        acc.items.push(machine);

        let mut profiles = base_item(
            item_id("inventory", None, "profiles"),
            Category::SystemInventory,
            "User profile list",
            "List of local profiles with account name, SID, profile path and last use (privacy notice: last-use data is informational).",
            SourceRef::Config { reference: r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\ProfileList".into() },
            None,
            RestoreKind::Inventory { name: "profiles".into() },
            ItemPayload::Inventory { name: "profiles".into() },
        );
        profiles.support = SupportLevel::InventoryOnly;
        profiles.item_count = Some(users.len() as u64);
        profiles.access = AccessState::Accessible;
        profiles.includes = vec!["Account names, SIDs, profile paths, approximate last use".into()];
        profiles.excludes = vec!["Passwords, password hashes, registry hives".into()];
        profiles.warnings.push(Warning::info(WarningCode::PrivacySensitive, "Last-use times are shown for planning only and are labelled with a confidence level."));
        apply_default_selection(&mut profiles, None);
        acc.items.push(profiles);

        let denied: Vec<_> = users.iter().filter(|u| !u.is_system_account && u.access == AccessState::AccessDenied).collect();
        for u in denied {
            let mut item = base_item(
                item_id("profile", Some(u), "inaccessible"),
                Category::UsersFiles,
                format!("{} (profile not accessible)", u.account_name),
                "This profile cannot be read with the current permissions. Restart elevated to include it.",
                path_source(&u.profile_path),
                Some(u),
                RestoreKind::Inventory { name: "profiles".into() },
                ItemPayload::None,
            );
            item.support = SupportLevel::Unsupported;
            item.access = AccessState::AccessDenied;
            item.requires_admin = true;
            item.warnings.push(Warning::warn(WarningCode::AdminRequired, "Administrator rights are required to read this profile. Access controls are never bypassed."));
            acc.items.push(item);
        }
        if ctx.platform.long_paths_enabled() == Some(false) {
            acc.warnings.push(Warning::info(WarningCode::LongPath, "Win32 long paths are not enabled on this PC. Migration Assistant handles long paths itself, but some apps may not open files with very long paths."));
        }
        Ok(())
    }
}

pub struct PrintersModule;

impl DiscoveryModule for PrintersModule {
    fn id(&self) -> &'static str {
        "printers"
    }
    fn stage(&self) -> &'static str {
        "Reading printer inventory"
    }
    fn discover(&self, ctx: &DiscoveryContext, _users: &[UserProfile], acc: &mut ScanAccumulator) -> AppResult<()> {
        let printers = match ctx.platform.printers() {
            Ok(p) => p,
            Err(e) => {
                acc.warnings.push(Warning::warn(WarningCode::AdapterUnavailable, format!("Printer inventory unavailable: {e}")));
                return Ok(());
            }
        };
        for p in &printers {
            let (support, notes, desc): (SupportLevel, Vec<String>, &str) = match p.connection {
                PrinterConnection::Shared => (
                    SupportLevel::Supported,
                    vec![format!("Reconnect to {} after confirmation. Windows may download the driver from the print server per its policy.", p.unc_path.clone().unwrap_or_else(|| p.name.clone()))],
                    "Shared printer connection (UNC).",
                ),
                PrinterConnection::Network => (
                    SupportLevel::Partial,
                    vec![
                        format!("Recreate a TCP/IP port for {} and add the printer only if the driver \"{}\" is already installed on the destination.", p.host_address.clone().unwrap_or_default(), p.driver_name),
                        "Drivers are never installed or copied automatically.".into(),
                    ],
                    "Network (TCP/IP) printer.",
                ),
                PrinterConnection::Virtual => (SupportLevel::InventoryOnly, vec!["Built-in virtual printer; present on the destination already.".into()], "Virtual printer (PDF/XPS/OneNote/Fax)."),
                _ => (
                    SupportLevel::InventoryOnly,
                    vec!["Connect the device to the destination PC and install the manufacturer's driver. A checklist is included in the report.".into()],
                    "Locally attached printer (USB/WSD/other).",
                ),
            };
            let mut item = base_item(
                item_id("printer", None, &p.name),
                Category::Printers,
                p.name.clone(),
                desc,
                SourceRef::Config { reference: format!("Printer queue: {} (port {})", p.name, p.port_name) },
                None,
                RestoreKind::Printers,
                ItemPayload::Inventory { name: "printers".into() },
            );
            item.support = support;
            item.access = AccessState::Accessible;
            item.item_count = Some(1);
            item.restore_notes = notes;
            item.includes = vec!["Name, share, port, host/IP, driver name/version, default flag, status (as JSON)".into()];
            item.excludes = vec!["Driver binaries, print queue jobs, any stored credentials".into()];
            if p.connection == PrinterConnection::Network || p.connection == PrinterConnection::Usb {
                item.warnings.push(Warning::info(WarningCode::DriverRequired, format!("Requires driver \"{}\" on the destination PC.", p.driver_name)));
            }
            if p.connection == PrinterConnection::Network {
                item.requires_admin = true;
            }
            apply_default_selection(&mut item, None);
            if p.connection == PrinterConnection::Virtual {
                item.selected_by_default = false;
            }
            acc.items.push(item);
        }
        acc.printers = printers;
        Ok(())
    }
}

pub struct NetworkDrivesModule;

impl DiscoveryModule for NetworkDrivesModule {
    fn id(&self) -> &'static str {
        "drives"
    }
    fn stage(&self) -> &'static str {
        "Reading mapped network drives"
    }
    fn discover(&self, ctx: &DiscoveryContext, _users: &[UserProfile], acc: &mut ScanAccumulator) -> AppResult<()> {
        let drives = match ctx.platform.mapped_drives() {
            Ok(d) => d,
            Err(e) => {
                acc.warnings.push(Warning::warn(WarningCode::AdapterUnavailable, format!("Mapped drive inventory unavailable: {e}")));
                return Ok(());
            }
        };
        for d in &drives {
            let mut item = base_item(
                item_id("drive", None, &d.letter),
                Category::NetworkDrives,
                format!("{} → {}", d.letter, d.unc_path),
                "Mapped network drive (current user).",
                SourceRef::Config { reference: format!("{} {}", d.letter, d.unc_path) },
                None,
                RestoreKind::MappedDrives,
                ItemPayload::Inventory { name: "network-drives".into() },
            );
            item.access = AccessState::Accessible;
            item.item_count = Some(1);
            item.includes = vec!["Drive letter, UNC path, provider, reconnect-at-sign-in flag, label".into()];
            item.excludes = vec!["Credentials: Windows prompts for them on the destination if needed".into()];
            item.restore_notes = vec![format!(
                "Recreate {} → {}{} after confirmation.",
                d.letter,
                d.unc_path,
                if d.persistent { " (reconnect at sign-in)" } else { "" }
            )];
            item.warnings.push(Warning::info(WarningCode::CredentialsNotMigrated, "Saved credentials are not migrated; Windows will ask for them if the share requires it."));
            apply_default_selection(&mut item, None);
            acc.items.push(item);
        }
        acc.mapped_drives = drives;
        Ok(())
    }
}
