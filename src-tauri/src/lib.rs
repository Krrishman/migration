//! Migration Assistant core library.
//!
//! Layering (outer layers depend on inner ones, never the reverse):
//! `commands` (Tauri IPC) → `capture` / `restore` / `reporting` →
//! `discovery` → `platform` (+ `windows` adapter) → `security`, `models`, `util`.

pub mod app_paths;
pub mod bundle;
pub mod capture;
pub mod discovery;
pub mod error;
pub mod fs_walk;
pub mod index;
pub mod models;
pub mod platform;
pub mod progress;
pub mod reporting;
pub mod restore;
pub mod security;
pub mod session;
pub mod util;
pub mod windows;

#[cfg(feature = "desktop")]
pub mod commands;

pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
