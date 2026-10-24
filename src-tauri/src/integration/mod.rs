//! Context-menu integration with the operating system (CLAUDE.md §6).

#[cfg(target_os = "macos")]
pub mod macos;
// Used by `macos`; compiled everywhere so its tests run on every platform.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub mod services;
#[cfg(target_os = "windows")]
pub mod windows;

use tauri::AppHandle;

/// Runs on every start: makes sure "Per QR-Code senden" is available. Runs
/// on a background thread: it starts helper processes and touches files,
/// which must not delay the first window (CLAUDE.md §12).
pub fn on_startup(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    {
        let app = app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            if let Err(e) = macos::ensure_quick_action(&app) {
                log::warn!("Schnelle Aktion nicht installiert: {e}");
            }
        });
    }
    #[cfg(target_os = "windows")]
    {
        let _ = app; // the registry is all Windows needs
        tauri::async_runtime::spawn_blocking(windows::ensure_context_menu);
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let _ = app;
}
