//! Headless demo: scan a fixture PC, capture a bundle, optionally restore it
//! onto a fixture target. No Tauri/WebView required.
//!
//!   cargo run --no-default-features --example fixture_demo -- \
//!       ../fixtures/source-pc /tmp/usb [../fixtures/target-pc]
//!
//! Fixture folders are modified only by the optional restore step, so pass a
//! copy of the target fixture if you want to keep the original pristine.

use migration_assistant_lib::capture::CaptureEngine;
use migration_assistant_lib::discovery::DiscoveryService;
use migration_assistant_lib::models::*;
use migration_assistant_lib::platform::fixture::FixturePlatform;
use migration_assistant_lib::platform::Platform;
use migration_assistant_lib::progress::NullSink;
use migration_assistant_lib::restore::{default_mappings, RestoreService};
use migration_assistant_lib::util::{format_bytes, CancelToken};
use std::path::PathBuf;
use std::sync::Arc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        eprintln!("usage: fixture_demo <source-fixture> <destination-root> [target-fixture]");
        std::process::exit(2);
    }
    let source = FixturePlatform::load(&PathBuf::from(&args[0]))?;
    let scan = DiscoveryService::new().scan(&source, &CancelToken::new(), true, &|p| eprintln!("[scan {}/{}] {}", p.stage_index + 1, p.stage_count, p.message))?;
    println!("Scanned {}: {} items, {} users", scan.machine.computer_name, scan.items.len(), scan.users.len());
    let mut ids: Vec<String> = scan.items.iter().filter(|i| i.selected_by_default).map(|i| i.id.clone()).collect();
    for i in &scan.items {
        if (i.id.starts_with("pst:") || i.id.starts_with("plugin-")) && !ids.contains(&i.id) {
            ids.push(i.id.clone());
        }
    }
    std::fs::create_dir_all(&args[1])?;
    let req = CaptureRequest { destination_root: args[1].clone(), selected_item_ids: ids, encryption: EncryptionRequest::default(), options: CaptureOptions::default() };
    let summary = CaptureEngine { platform: &source, sink: Arc::new(NullSink), cancel: CancelToken::new() }.start(&scan, &req)?;
    println!("Bundle: {}", summary.bundle_path);
    println!("Status: {:?}, verified: {}, {} files, {}", summary.status, summary.verified, summary.total_files, format_bytes(summary.total_bytes));
    println!("Report: {}", summary.report_html);

    if let Some(t) = args.get(2) {
        let target = FixturePlatform::load(&PathBuf::from(t))?;
        let svc = RestoreService { platform: &target, sink: Arc::new(NullSink), cancel: CancelToken::new() };
        let mut opened = svc.open_bundle(std::path::Path::new(&summary.bundle_path), true, |_| {})?;
        let req = RestoreRequest {
            bundle_path: summary.bundle_path.clone(),
            mappings: default_mappings(&opened.manifest, &target.list_profiles()?),
            selected_item_ids: opened.manifest.items.iter().map(|i| i.id.clone()).collect(),
            policies: Default::default(),
            replace_confirmed: vec![],
            confirmed_categories: Category::ALL.to_vec(),
            passphrase: None,
        };
        let plan = svc.plan(&opened, &req)?;
        println!("Restore plan: {} action(s), {} file(s), {} conflict(s)", plan.actions.len(), plan.total_files, plan.total_conflicts);
        let r = svc.execute(&mut opened, &req)?;
        println!("Restore: {} — {} written, {} skipped, {} failures. Report: {}", r.outcome, r.files_written, r.files_skipped, r.failures, r.report_html);
    }
    Ok(())
}
