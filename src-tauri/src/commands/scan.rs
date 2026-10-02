//! Source scan commands.

use super::{blocking, ScanCancel, SessionState};
use crate::error::AppError;
use crate::models::{DiscoveryItem, ScanResult};
use crate::util::CancelToken;
use tauri::Emitter;

#[tauri::command]
pub async fn start_scan(app: tauri::AppHandle, state: SessionState<'_>, cancel: tauri::State<'_, ScanCancel>) -> Result<ScanResult, AppError> {
    let token = CancelToken::new();
    *cancel.0.lock() = Some(token.clone());
    let s = state.inner().clone();
    let r = blocking(move || {
        s.scan(&token, &move |p| {
            let _ = app.emit("scan://progress", p);
        })
    })
    .await;
    *cancel.0.lock() = None;
    r
}

#[tauri::command]
pub fn cancel_scan(cancel: tauri::State<'_, ScanCancel>) -> bool {
    cancel.0.lock().as_ref().map(|t| t.cancel()).is_some()
}

#[tauri::command]
pub fn get_scan(state: SessionState<'_>) -> Option<ScanResult> {
    state.current_scan()
}

#[tauri::command]
pub async fn add_custom_folder(state: SessionState<'_>, path: String, owner_sid: Option<String>) -> Result<DiscoveryItem, AppError> {
    let s = state.inner().clone();
    blocking(move || s.add_custom_folder(&path, owner_sid.as_deref())).await
}

#[tauri::command]
pub async fn add_chromium_root(state: SessionState<'_>, path: String, name: String, owner_sid: String) -> Result<Vec<DiscoveryItem>, AppError> {
    let s = state.inner().clone();
    blocking(move || s.add_chromium_root(&path, &name, &owner_sid)).await
}
