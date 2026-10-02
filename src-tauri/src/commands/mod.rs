//! Tauri IPC layer. Commands are thin: they validate nothing themselves,
//! delegate to [`Session`], run blocking work off the UI thread, and forward
//! progress as events:
//!
//! * `scan://progress`            – [`ScanProgress`]
//! * `capture://progress|log`     – [`TaskProgress`] / [`LogEntry`]
//! * `restore://progress|log`     – [`TaskProgress`] / [`LogEntry`]
//! * `restore://verify`           – bundle-relative path being verified

mod app;
mod capture;
mod restore;
mod scan;

use crate::error::AppError;
use crate::models::{LogEntry, TaskProgress};
use crate::progress::ProgressSink;
use crate::session::Session;
use crate::util::CancelToken;
use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};

pub type SessionState<'a> = tauri::State<'a, Arc<Session>>;

#[derive(Default)]
pub struct ScanCancel(pub Mutex<Option<CancelToken>>);

/// Forwards engine progress to the webview.
pub struct TauriSink {
    pub app: AppHandle,
    pub channel: &'static str,
}

impl ProgressSink for TauriSink {
    fn task(&self, p: &TaskProgress) {
        let _ = self.app.emit(&format!("{}://progress", self.channel), p);
    }
    fn log(&self, e: &LogEntry) {
        let _ = self.app.emit(&format!("{}://log", self.channel), e);
    }
}

/// Run blocking work on the async runtime's blocking pool.
pub async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, AppError> + Send + 'static) -> Result<T, AppError> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| AppError::InvalidRequest(format!("background task failed: {e}")))?
}

fn fixture_arg() -> Option<PathBuf> {
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--fixture" {
            return args.next().map(PathBuf::from);
        }
        if let Some(v) = a.strip_prefix("--fixture=") {
            return Some(PathBuf::from(v));
        }
    }
    std::env::var_os("MIGRATION_ASSISTANT_FIXTURE").map(PathBuf::from)
}

pub fn run() {
    let platform = match crate::platform::select_platform(fixture_arg()) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Migration Assistant cannot start: {e}");
            std::process::exit(1);
        }
    };
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("migration-assistant"));
    let session = Arc::new(Session::new(platform, &exe));
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(session)
        .manage(ScanCancel::default())
        .invoke_handler(tauri::generate_handler![
            app::app_status,
            app::acknowledge_authorization,
            app::set_destination,
            app::save_config,
            app::disk_space,
            app::restart_elevated,
            app::open_path,
            app::list_bundles,
            app::load_report,
            scan::start_scan,
            scan::cancel_scan,
            scan::get_scan,
            scan::add_custom_folder,
            scan::add_chromium_root,
            capture::capture_preflight,
            capture::start_capture,
            capture::cancel_capture,
            capture::resume_capture,
            capture::delete_incomplete_bundle,
            capture::prepare_restore_instructions,
            restore::open_bundle,
            restore::target_info,
            restore::plan_restore,
            restore::execute_restore,
            restore::cancel_restore,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Migration Assistant");
}
