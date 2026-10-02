//! Restore commands.

use super::{blocking, SessionState, TauriSink};
use crate::error::AppError;
use crate::models::{RestorePlan, RestoreRequest, RestoreSummary, TargetInfo};
use crate::session::BundleOverview;
use std::sync::Arc;
use tauri::Emitter;

#[tauri::command]
pub async fn open_bundle(app: tauri::AppHandle, state: SessionState<'_>, path: String) -> Result<BundleOverview, AppError> {
    let s = state.inner().clone();
    blocking(move || {
        let mut n = 0u64;
        s.open_bundle(&path, |f| {
            n += 1;
            if n % 25 == 1 {
                let _ = app.emit("restore://verify", (n, f.to_string()));
            }
        })
    })
    .await
}

#[tauri::command]
pub async fn target_info(state: SessionState<'_>) -> Result<TargetInfo, AppError> {
    let s = state.inner().clone();
    blocking(move || s.target_info()).await
}

#[tauri::command]
pub async fn plan_restore(state: SessionState<'_>, request: RestoreRequest) -> Result<RestorePlan, AppError> {
    let s = state.inner().clone();
    blocking(move || s.plan_restore(&request)).await
}

#[tauri::command]
pub async fn execute_restore(app: tauri::AppHandle, state: SessionState<'_>, request: RestoreRequest) -> Result<RestoreSummary, AppError> {
    let s = state.inner().clone();
    let sink = Arc::new(TauriSink { app, channel: "restore" });
    blocking(move || s.execute_restore(&request, sink)).await
}

#[tauri::command]
pub fn cancel_restore(state: SessionState<'_>) -> bool {
    state.cancel_restore()
}
