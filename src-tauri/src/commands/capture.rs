//! Capture commands.

use super::{blocking, SessionState, TauriSink};
use crate::error::AppError;
use crate::models::{CaptureRequest, CaptureSummary, PreflightReport};
use std::sync::Arc;

#[tauri::command]
pub async fn capture_preflight(state: SessionState<'_>, request: CaptureRequest) -> Result<PreflightReport, AppError> {
    let s = state.inner().clone();
    blocking(move || s.preflight(&request)).await
}

#[tauri::command]
pub async fn start_capture(app: tauri::AppHandle, state: SessionState<'_>, request: CaptureRequest) -> Result<CaptureSummary, AppError> {
    let s = state.inner().clone();
    let sink = Arc::new(TauriSink { app, channel: "capture" });
    blocking(move || s.capture(&request, sink)).await
}

#[tauri::command]
pub fn cancel_capture(state: SessionState<'_>) -> bool {
    state.cancel_capture()
}

#[tauri::command]
pub async fn resume_capture(app: tauri::AppHandle, state: SessionState<'_>, bundle_path: String, passphrase: Option<String>) -> Result<CaptureSummary, AppError> {
    let s = state.inner().clone();
    let sink = Arc::new(TauriSink { app, channel: "capture" });
    blocking(move || s.resume_capture(&bundle_path, passphrase.as_deref(), sink)).await
}

#[tauri::command]
pub async fn delete_incomplete_bundle(state: SessionState<'_>, bundle_path: String, confirm_bundle_id: String) -> Result<(), AppError> {
    let s = state.inner().clone();
    blocking(move || s.delete_incomplete_bundle(&bundle_path, &confirm_bundle_id)).await
}

#[tauri::command]
pub async fn prepare_restore_instructions(state: SessionState<'_>, bundle_path: String) -> Result<String, AppError> {
    let s = state.inner().clone();
    blocking(move || s.prepare_restore_instructions(&bundle_path)).await
}
