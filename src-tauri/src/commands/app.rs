//! Application-level commands: status, authorization, paths, reports.

use super::{blocking, SessionState};
use crate::error::AppError;
use crate::index::IndexedBundle;
use crate::models::DiskSpace;
use crate::session::{AppStatus, ReportView};

#[tauri::command]
pub fn app_status(state: SessionState<'_>) -> AppStatus {
    state.status()
}

#[tauri::command]
pub fn acknowledge_authorization(state: SessionState<'_>) -> Result<(), AppError> {
    state.acknowledge_authorization()
}

#[tauri::command]
pub fn set_destination(state: SessionState<'_>, path: String) -> Result<String, AppError> {
    state.set_destination(&path)
}

#[tauri::command]
pub fn save_config(state: SessionState<'_>) -> Result<String, AppError> {
    state.save_config()
}

#[tauri::command]
pub fn disk_space(state: SessionState<'_>, path: String) -> Option<DiskSpace> {
    state.disk_space(&path)
}

#[tauri::command]
pub fn restart_elevated(app: tauri::AppHandle, state: SessionState<'_>, modules: Vec<String>) -> Result<(), AppError> {
    state.restart_elevated(&modules)?;
    // The elevated instance takes over; this one exits cleanly.
    app.exit(0);
    Ok(())
}

#[tauri::command]
pub fn open_path(state: SessionState<'_>, path: String) -> Result<(), AppError> {
    state.open_path(&path)
}

#[tauri::command]
pub fn list_bundles(state: SessionState<'_>) -> Vec<IndexedBundle> {
    state.list_bundles()
}

#[tauri::command]
pub async fn load_report(state: SessionState<'_>, path: String) -> Result<ReportView, AppError> {
    let s = state.inner().clone();
    blocking(move || s.load_report(&path)).await
}
