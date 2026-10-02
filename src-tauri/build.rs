fn main() {
    // Tauri's build step (resource embedding, Windows manifest, icons) only runs
    // for the desktop build. Library-only builds and tests skip it entirely.
    #[cfg(feature = "desktop")]
    tauri_build::build();
}
