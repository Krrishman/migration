//! Report generation: technician HTML, machine-readable JSON, and a
//! redacted end-user summary. Reports are written into the bundle (local by
//! default) and never contain secrets.

pub mod html;

use crate::bundle::BundleLayout;
use crate::error::AppResult;
use crate::models::*;
use crate::security::redaction::Redactor;
use crate::security::safe_path::display_path;
use crate::util::{format_bytes, write_bytes_atomic, write_json_atomic};
use html::Html;
use serde::Serialize;

pub struct ReportPaths {
    pub technician_html: String,
    pub json: String,
    pub summary_html: String,
}

const NO_SECRETS: &str = "This report never contains passwords, browser cookies, authentication tokens, saved credentials, Wi-Fi keys, private keys or license keys.";

fn opt_bytes(b: Option<u64>) -> String {
    b.map(format_bytes).unwrap_or_else(|| "unknown".into())
}

fn status_label(s: BundleStatus) -> &'static str {
    match s {
        BundleStatus::InProgress => "In progress (incomplete)",
        BundleStatus::Canceled => "Canceled (incomplete, resumable)",
        BundleStatus::CompletedUnverified => "Completed — NOT verified",
        BundleStatus::Verified => "Verified",
        BundleStatus::VerifiedWithWarnings => "Verified with warnings",
    }
}

fn capture_label(s: CaptureStatus) -> &'static str {
    match s {
        CaptureStatus::Pending => "Not captured",
        CaptureStatus::Captured => "Captured",
        CaptureStatus::CapturedWithWarnings => "Captured with warnings",
        CaptureStatus::Skipped => "Skipped",
        CaptureStatus::Failed => "Failed",
        CaptureStatus::Canceled => "Canceled",
    }
}

fn support_label(s: SupportLevel) -> &'static str {
    match s {
        SupportLevel::Supported => "Supported",
        SupportLevel::Partial => "Partial",
        SupportLevel::InventoryOnly => "Inventory only",
        SupportLevel::Unsupported => "Unsupported",
    }
}

pub fn restore_instructions(m: &Manifest) -> Vec<String> {
    let mut v = vec![
        "Copy or connect this bundle folder to the destination PC (keep the folder structure intact).".to_string(),
        "Run Migration Assistant on the destination PC and choose \"Restore migration backup\".".into(),
        "Select this bundle folder or its manifest.json; integrity is verified before any restore option is shown.".into(),
    ];
    if m.encryption.enabled {
        v.push("Enter the bundle passphrase. Without it the encrypted files cannot be restored.".into());
    }
    v.push("Map each source user to a destination user, review the dry-run plan and collision policy, then confirm.".into());
    if !m.applications.is_empty() {
        v.push("Reinstall applications from inventory/reinstall-checklist.txt before restoring application settings.".into());
    }
    if !m.printers.is_empty() {
        v.push("Install printer drivers on the destination before restoring network printers.".into());
    }
    if !m.browser_profiles.is_empty() {
        v.push("Close browsers before restoring. Sign in to browser sync to bring back passwords; they are not in the bundle.".into());
    }
    v
}

#[derive(Serialize)]
struct JsonReport<'a> {
    report_type: &'static str,
    generated_at: chrono::DateTime<chrono::Utc>,
    app_version: &'static str,
    bundle_id: &'a str,
    status: BundleStatus,
    scope: Vec<String>,
    restore_instructions: Vec<String>,
    task_results: &'a [TaskProgress],
    manifest: &'a Manifest,
}

pub fn technician_html(m: &Manifest, tasks: &[TaskProgress]) -> String {
    let mut h = Html::page("Migration Assistant — Technician Report");
    h.h1("Migration Assistant — Technician Report");
    h.muted(&format!(
        "Bundle {} · created {} · app version {} · capture status: {}",
        m.bundle_id,
        m.created_at.format("%Y-%m-%d %H:%M:%S UTC"),
        m.app_version,
        status_label(m.status)
    ));
    h.kv(&[
        ("Computer", m.source_machine.computer_name.clone()),
        ("Windows", format!("{} {}", m.source_machine.os_name, m.source_machine.os_version)),
        ("Build / architecture", format!("{} / {}", m.source_machine.os_build, m.source_machine.architecture)),
        ("Time zone", m.source_machine.time_zone.clone()),
        ("Join state", format!("{:?}{}", m.source_machine.join_state, m.source_machine.join_name.as_ref().map(|n| format!(" ({n})")).unwrap_or_default())),
        ("Elevated during capture", if m.elevated { "Yes".into() } else { "No".into() }),
        ("Encryption at rest", if m.encryption.enabled { format!("On ({})", m.encryption.algorithm.clone().unwrap_or_default()) } else { "Off".into() }),
        ("Destination root", m.destination_root.clone()),
        ("Captured", format!("{} files, {}", m.integrity.total_files, format_bytes(m.integrity.total_bytes))),
        ("Destination free space at capture", opt_bytes(m.capacity.destination_free_bytes)),
        ("Source free space at capture", opt_bytes(m.capacity.source_free_bytes)),
        ("Device ID", m.source_machine.device_id.clone().unwrap_or_else(|| "Not collected".into())),
    ]);

    h.h2("Explicit scope");
    h.list(&m.items.iter().map(|i| format!("{} — {} ({})", i.display_name, support_label(i.support), capture_label(i.capture_status))).collect::<Vec<_>>());

    h.h2("Users");
    h.table(
        &["Account", "SID", "Profile path", "Last use", "Approx. size", "Bundle folder"],
        &m.users
            .iter()
            .map(|u| {
                vec![
                    u.account_name.clone(),
                    u.sid.clone(),
                    u.profile_path.clone(),
                    u.last_use.map(|d| d.format("%Y-%m-%d").to_string()).unwrap_or_else(|| "unknown".into()),
                    opt_bytes(u.profile_size),
                    u.bundle_dir.clone(),
                ]
            })
            .collect::<Vec<_>>(),
    );

    h.h2("Captured items");
    h.table(
        &["Category", "Item", "Owner", "Source", "Files", "Size", "Status", "Hash", "Warnings"],
        &m.items
            .iter()
            .map(|i| {
                vec![
                    i.category.label().into(),
                    i.display_name.clone(),
                    i.owner.as_ref().map(|o| o.account_name.clone()).unwrap_or_else(|| "Computer".into()),
                    i.source_path.clone(),
                    i.captured_files.to_string(),
                    format_bytes(i.captured_bytes),
                    capture_label(i.capture_status).into(),
                    format!("{:?}", i.hash_status),
                    i.warnings.len().to_string(),
                ]
            })
            .collect::<Vec<_>>(),
    );

    if !tasks.is_empty() {
        h.h2("Task results");
        h.table(
            &["Task", "Result", "Items", "Bytes", "Retries", "Warnings", "Error"],
            &tasks
                .iter()
                .map(|t| {
                    vec![
                        t.display_name.clone(),
                        format!("{:?}", t.state),
                        t.items_done.to_string(),
                        format_bytes(t.bytes_done),
                        t.retry_count.to_string(),
                        t.warning_count.to_string(),
                        t.error.clone().unwrap_or_default(),
                    ]
                })
                .collect::<Vec<_>>(),
        );
    }

    h.h2("Skipped items, warnings and errors");
    let mut warn_rows = Vec::new();
    for i in &m.items {
        for w in &i.warnings {
            warn_rows.push(vec![format!("{:?}", w.severity), i.display_name.clone(), w.message.clone(), w.path.clone().unwrap_or_default()]);
        }
    }
    h.table(&["Severity", "Item", "Message", "Path"], &warn_rows);
    h.muted(&format!(
        "Log: {} — {} info, {} warning(s), {} error(s).",
        m.log_summary.log_file, m.log_summary.info_count, m.log_summary.warning_count, m.log_summary.error_count
    ));
    if !m.log_summary.errors.is_empty() {
        h.list(&m.log_summary.errors);
    }

    h.h2("Excluded by policy");
    h.details("Show exclusion rules", |h| {
        h.table(&["Pattern", "Reason"], &m.exclusions.iter().map(|e| vec![e.pattern.clone(), e.reason.clone()]).collect::<Vec<_>>());
    });

    h.h2("Browser profiles");
    h.muted("Profile names only. No passwords, cookies, tokens or payment data are captured.");
    h.table(
        &["Browser", "User", "Profile folder", "Profile name", "Size"],
        &m.browser_profiles
            .iter()
            .map(|b| vec![b.browser.label().into(), b.owner.account_name.clone(), b.profile_dir.clone(), b.profile_name.clone().unwrap_or_default(), opt_bytes(b.size_bytes)])
            .collect::<Vec<_>>(),
    );

    h.h2("Printers");
    h.table(
        &["Name", "Connection", "Port", "Host / UNC", "Driver", "Default", "Status"],
        &m.printers
            .iter()
            .map(|p| {
                vec![
                    p.name.clone(),
                    format!("{:?}", p.connection),
                    p.port_name.clone(),
                    p.unc_path.clone().or(p.host_address.clone()).unwrap_or_default(),
                    format!("{}{}", p.driver_name, p.driver_version.as_ref().map(|v| format!(" ({v})")).unwrap_or_default()),
                    if p.is_default { "Yes".into() } else { String::new() },
                    p.status.clone(),
                ]
            })
            .collect::<Vec<_>>(),
    );
    let local: Vec<String> = m
        .printers
        .iter()
        .filter(|p| matches!(p.connection, PrinterConnection::Usb | PrinterConnection::Local | PrinterConnection::Wsd | PrinterConnection::Other))
        .map(|p| format!("[ ] {}: connect the device, install \"{}\" from the manufacturer, print a test page.", p.name, p.driver_name))
        .collect();
    if !local.is_empty() {
        h.p("Local printer restore checklist:");
        h.list(&local);
    }

    h.h2("Network drives");
    h.table(
        &["Drive", "UNC path", "Reconnect at sign-in", "Status"],
        &m.mapped_drives.iter().map(|d| vec![d.letter.clone(), d.unc_path.clone(), if d.persistent { "Yes".into() } else { "No".into() }, d.status.clone()]).collect::<Vec<_>>(),
    );

    h.h2("Installed applications");
    h.table(
        &["Name", "Version", "Publisher", "Category", "Arch", "Install date", "Settings plug-in", "Description"],
        &m.applications
            .iter()
            .map(|a| {
                vec![
                    a.display_name.clone(),
                    a.version.clone().unwrap_or_default(),
                    a.publisher.clone().unwrap_or_default(),
                    a.category.clone(),
                    a.architecture.clone(),
                    a.install_date.clone().unwrap_or_default(),
                    a.settings_plugin.clone().unwrap_or_default(),
                    a.description.clone().unwrap_or_else(|| "Unknown application.".into()),
                ]
            })
            .collect::<Vec<_>>(),
    );
    if m.applications.iter().any(|a| a.uninstall_command.is_some()) {
        h.details("Advanced (sensitive): uninstall commands — technician use only", |h| {
            h.table(
                &["Name", "Uninstall command"],
                &m.applications.iter().filter_map(|a| a.uninstall_command.as_ref().map(|u| vec![a.display_name.clone(), u.clone()])).collect::<Vec<_>>(),
            );
        });
    }

    h.h2("Integrity verification");
    h.kv(&[
        ("Verified", if m.integrity.verified { "Yes".into() } else { "No".into() }),
        ("Verified at", m.integrity.verified_at.map(|d| d.to_rfc3339()).unwrap_or_else(|| "—".into())),
        ("Algorithm", m.integrity.hash_algorithm.clone()),
        ("Files / bytes", format!("{} / {}", m.integrity.total_files, format_bytes(m.integrity.total_bytes))),
        ("Mismatches", m.integrity.mismatches.to_string()),
        ("Bundle root hash", m.integrity.bundle_root_hash.clone().unwrap_or_default()),
    ]);
    h.muted(&format!("Strategy: {}", m.integrity.strategy));

    h.h2("Restore instructions");
    h.list(&restore_instructions(m));
    let notes: Vec<String> = m.items.iter().flat_map(|i| i.restore_notes.iter().map(move |n| format!("{}: {n}", i.display_name))).collect();
    h.details("Per-item restore notes", |h| {
        h.list(&notes);
    });
    h.h2("Restore compatibility notes");
    h.list(&m.restore_compatibility_notes);
    if !m.restore_history.is_empty() {
        h.h2("Restore history");
        h.table(
            &["Restore", "Finished", "Target", "Written", "Skipped", "Failures", "Outcome"],
            &m.restore_history
                .iter()
                .map(|r| vec![r.restore_id.clone(), r.finished_at.to_rfc3339(), r.target_computer.clone(), r.files_written.to_string(), r.files_skipped.to_string(), r.failures.to_string(), r.outcome.clone()])
                .collect::<Vec<_>>(),
        );
    }
    h.finish(&format!("Generated {} by Migration Assistant {}. {NO_SECRETS} Keep this detailed report local.", chrono::Utc::now().format("%Y-%m-%d %H:%M UTC"), crate::APP_VERSION))
}

pub fn redactor_for(m: &Manifest) -> Redactor {
    let mut r = Redactor::new().computer_name(&m.source_machine.computer_name);
    for u in &m.users {
        r = r.user(&u.account_name, &u.profile_path);
    }
    if let Some(n) = &m.source_machine.join_name {
        r = r.term(n, "[domain]");
    }
    r
}

/// Concise end-user summary with users, computer, paths and servers redacted.
pub fn summary_html(m: &Manifest) -> String {
    let r = redactor_for(m);
    let mut h = Html::page("Migration Summary");
    h.h1("Migration summary");
    h.muted(&r.redact(&format!(
        "Bundle {} · captured {} · Migration Assistant {} · status: {}",
        m.bundle_id,
        m.created_at.format("%Y-%m-%d %H:%M UTC"),
        m.app_version,
        status_label(m.status)
    )));
    h.kv(&[
        ("Computer", r.redact(&m.source_machine.computer_name)),
        ("Windows", format!("{} {}", m.source_machine.os_name, m.source_machine.os_version)),
        ("Users included", m.users.len().to_string()),
        ("Total size", format_bytes(m.integrity.total_bytes)),
        ("Files", m.integrity.total_files.to_string()),
        ("Integrity", if m.integrity.verified { "Verified".into() } else { "Not verified".into() }),
        ("Encrypted", if m.encryption.enabled { "Yes".into() } else { "No".into() }),
    ]);
    h.h2("What was included");
    let mut by_cat: std::collections::BTreeMap<Category, (u64, u64, usize)> = Default::default();
    for i in &m.items {
        let e = by_cat.entry(i.category).or_default();
        e.0 += i.captured_files;
        e.1 += i.captured_bytes;
        e.2 += 1;
    }
    h.table(
        &["Category", "Items", "Files", "Size"],
        &by_cat.iter().map(|(c, (f, b, n))| vec![c.label().into(), n.to_string(), f.to_string(), format_bytes(*b)]).collect::<Vec<_>>(),
    );
    h.h2("Needs attention");
    let mut msgs: Vec<String> = m
        .items
        .iter()
        .flat_map(|i| i.warnings.iter().filter(|w| w.severity >= Severity::Warning).map(move |w| format!("{}: {}", i.display_name, w.message)))
        .map(|s| r.redact(&s))
        .collect();
    msgs.sort();
    msgs.dedup();
    h.list(&msgs);
    h.h2("Not migrated by design");
    h.list(&[
        "Saved passwords, browser cookies and sign-ins (use browser/account sync)".to_string(),
        "Installed programs and license keys (reinstall from official sources)".into(),
        "Wi-Fi passwords and stored network credentials".into(),
    ]);
    h.finish(&format!("Redacted summary suitable for sharing. {NO_SECRETS}"))
}

pub fn write_capture_reports(layout: &BundleLayout, m: &Manifest, tasks: &[TaskProgress]) -> AppResult<ReportPaths> {
    let tech = layout.root.join("report.html");
    write_bytes_atomic(&tech, technician_html(m, tasks).as_bytes())?;
    let json = layout.root.join("report.json");
    write_json_atomic(
        &json,
        &JsonReport {
            report_type: "capture",
            generated_at: chrono::Utc::now(),
            app_version: crate::APP_VERSION,
            bundle_id: &m.bundle_id,
            status: m.status,
            scope: m.items.iter().map(|i| i.display_name.clone()).collect(),
            restore_instructions: restore_instructions(m),
            task_results: tasks,
            manifest: m,
        },
    )?;
    let summary = layout.root.join("summary-report.html");
    write_bytes_atomic(&summary, summary_html(m).as_bytes())?;
    Ok(ReportPaths { technician_html: display_path(&tech), json: display_path(&json), summary_html: display_path(&summary) })
}

/// Restore report (HTML + JSON) for one restore run.
pub fn restore_report_html(m: &Manifest, plan: &RestorePlan, summary: &RestoreSummary, target: &str) -> String {
    let mut h = Html::page("Migration Assistant — Restore Report");
    h.h1("Migration Assistant — Restore Report");
    h.muted(&format!("Restore {} of bundle {} onto {} · app version {}", summary.restore_id, m.bundle_id, target, crate::APP_VERSION));
    h.kv(&[
        ("Source computer", m.source_machine.computer_name.clone()),
        ("Target computer", target.to_string()),
        ("Outcome", summary.outcome.clone()),
        ("Files written", summary.files_written.to_string()),
        ("Renamed (collision)", summary.files_renamed.to_string()),
        ("Replaced (previous copy kept as .bak)", summary.files_replaced.to_string()),
        ("Skipped", summary.files_skipped.to_string()),
        ("Failures", summary.failures.to_string()),
        ("Verified after copy", if summary.verified { "Yes".into() } else { "No".into() }),
    ]);
    h.h2("Actions");
    h.table(
        &["Category", "Item", "Source user", "Target user", "Target", "Files", "Conflicts", "Policy", "Result"],
        &plan
            .actions
            .iter()
            .map(|a| {
                let res = summary.task_results.iter().find(|t| t.task_id == a.id).map(|t| format!("{:?}", t.state)).unwrap_or_else(|| "Not run".into());
                vec![
                    a.category.label().into(),
                    a.display_name.clone(),
                    a.source_user.clone().unwrap_or_default(),
                    a.target_user.clone().unwrap_or_default(),
                    a.target_path.clone().unwrap_or_default(),
                    a.files.to_string(),
                    a.conflicts.to_string(),
                    format!("{:?}", a.policy),
                    res,
                ]
            })
            .collect::<Vec<_>>(),
    );
    h.h2("System changes applied or listed");
    let changes: Vec<String> = plan.actions.iter().flat_map(|a| a.system_changes.iter().map(|c| format!("{}: {}", a.display_name, describe_change(c)))).collect();
    h.list(&changes);
    h.h2("Warnings");
    h.list(&summary.warnings.iter().map(|w| format!("{}{}", w.message, w.path.as_ref().map(|p| format!(" ({p})")).unwrap_or_default())).collect::<Vec<_>>());
    h.finish(NO_SECRETS)
}

pub fn describe_change(c: &SystemChange) -> String {
    match c {
        SystemChange::RegistryValue { hive, key, value_name, value } => format!("Set {hive}\\{key}\\{value_name} = \"{value}\""),
        SystemChange::MapDrive { letter, unc_path, persistent } => format!("Map {letter} to {unc_path}{}", if *persistent { " (reconnect at sign-in)" } else { "" }),
        SystemChange::ConnectSharedPrinter { unc_path } => format!("Connect shared printer {unc_path}"),
        SystemChange::AddNetworkPrinter { name, host_address, driver_name, port_name } => {
            format!("Add printer \"{name}\" on port {port_name} ({host_address}) using installed driver \"{driver_name}\"")
        }
        SystemChange::ManualChecklist { text } => format!("Manual step: {text}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_redacts_identities() {
        let m: Manifest = serde_json::from_str(include_str!("../../../SAMPLE_MANIFEST.json")).unwrap();
        let html = summary_html(&m);
        for u in &m.users {
            let short = u.account_name.rsplit('\\').next().unwrap();
            assert!(!html.to_lowercase().contains(&short.to_lowercase()), "summary leaks {short}");
            assert!(!html.contains(&u.profile_path));
        }
        assert!(!html.contains(&m.source_machine.computer_name));
        let tech = technician_html(&m, &[]);
        assert!(tech.contains(&m.source_machine.computer_name));
        assert!(!tech.to_lowercase().contains("<script"));
    }
}
