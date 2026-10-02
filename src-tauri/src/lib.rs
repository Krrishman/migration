//! Migration Assistant core library.
//!
//! Layering (outer layers depend on inner ones, never the reverse):
//! `commands` (Tauri IPC) → `capture` / `restore` / `reporting` →
//! `discovery` → `platform` (+ `windows` adapter) → `security`, `models`, `util`.

pub mod bundle;
pub mod capture;
pub mod discovery;
pub mod error;
pub mod fs_walk;
pub mod models;
pub mod platform;
pub mod progress;
pub mod reporting;
pub mod security;
pub mod util;
pub mod windows;

pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
