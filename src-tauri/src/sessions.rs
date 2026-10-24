//! Starting share and receive sessions, each in its own window.

use std::path::PathBuf;
use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager};

use crate::app_core::{self, AppCore, ShareError};
use crate::share::{new_id, ShareSource};
use crate::ui::{self, SessionView};

/// A fresh session id; `None` (logged) if the system has no randomness.
fn new_session_id(app: &AppHandle) -> Option<String> {
    match new_id() {
        Ok(id) => Some(id),
        Err(e) => {
            log::error!("Keine Zufallszahlen verfügbar: {e}");
            ui::exit_if_idle(app);
            None
        }
    }
}

/// Common tail of every session start: opens its window (which also shows
/// an error state) and closes the drop zone. If the window cannot open, the
/// session is forgotten again. Returns whether the window is open.
fn open_session(app: &AppHandle, core: &AppCore, id: &str, view: SessionView) -> bool {
    if let Err(e) = ui::open_session_window(app, id, view) {
        log::error!("Fenster konnte nicht geöffnet werden: {e}");
        core.forget(id);
        ui::exit_if_idle(app);
        return false;
    }
    ui::close(app, ui::DROPZONE);
    true
}

fn display_name(paths: &[PathBuf]) -> String {
    paths
        .first()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Opens a share window right away (state "preparing") and scans the files
/// in the background: the window must appear within 500 ms (CLAUDE.md §12).
/// Returns the share id.
pub fn start_share(app: &AppHandle, paths: Vec<PathBuf>) -> Option<String> {
    if paths.is_empty() {
        return None;
    }
    let core = app_core::get(app)?;
    let id = new_session_id(app)?;
    core.set_pending(&id, display_name(&paths), paths.len());
    if !open_session(app, &core, &id, SessionView::Share) {
        return None;
    }
    log::info!("Neue Freigabe mit {} Pfad(en)", paths.len());
    spawn_scan(app.clone(), core, id.clone(), paths);
    Some(id)
}

/// The selection turned out bigger than its first file: the same window now
/// shares all paths (as ZIP) under a new QR code.
pub fn grow_share(app: &AppHandle, id: &str, paths: Vec<PathBuf>) {
    let Some(core) = app_core::get(app) else {
        return;
    };
    core.clear_failed(id);
    core.set_pending(id, display_name(&paths), paths.len());
    notify_update(app, id);
    log::info!("Freigabe auf {} Pfade erweitert", paths.len());
    spawn_scan(app.clone(), core, id.to_string(), paths);
}

/// Tells a session window to fetch its state again.
fn notify_update(app: &AppHandle, id: &str) {
    if let Err(e) = app.emit_to(ui::session_label(id), "share://update", ()) {
        log::debug!("Event nicht zugestellt: {e}");
    }
}

fn spawn_scan(app: AppHandle, core: Arc<AppCore>, id: String, paths: Vec<PathBuf>) {
    let generation = core.next_scan(&id);
    tauri::async_runtime::spawn(async move {
        let date = chrono::Local::now().format("%Y-%m-%d").to_string();
        let scan =
            tauri::async_runtime::spawn_blocking(move || ShareSource::from_paths(&paths, &date))
                .await;
        if !core.is_current_scan(&id, generation) {
            // Superseded by a bigger selection, or the window was closed
            // (which forgets the share).
            return;
        }
        match scan {
            Ok(Ok(source)) => {
                if let Err(e) = core.manager.create_with_id(id.clone(), source) {
                    log::error!("Freigabe fehlgeschlagen: {e}");
                    core.set_failed(&id, ShareError::NotFound);
                }
            }
            Ok(Err(e)) => {
                log::warn!("Dateien nicht lesbar: {}", e.kind());
                core.set_failed(&id, ShareError::NotFound);
            }
            Err(e) => {
                log::error!("Scan abgebrochen: {e}");
                core.set_failed(&id, ShareError::NotFound);
            }
        }
        core.clear_pending(&id);
        // Closed while the share was being created: cancelled.
        if !ui::has_session_window(&app, &id) {
            core.forget(&id);
            return;
        }
        notify_update(&app, &id);
    });
}

/// "Vom Handy empfangen": opens a receive window. Files land in the
/// Downloads folder, texts in the clipboard.
pub fn start_receive(app: &AppHandle) {
    let Some(core) = app_core::get(app) else {
        return;
    };
    let Some(id) = new_session_id(app) else {
        return;
    };
    let dir = app
        .path()
        .download_dir()
        .or_else(|_| app.path().home_dir())
        .unwrap_or_else(|_| std::env::temp_dir());
    // Leftovers of uploads that were cut off in an earlier run.
    {
        let dir = dir.clone();
        tauri::async_runtime::spawn_blocking(move || {
            crate::server::receive::remove_stale_parts(&dir)
        });
    }
    if let Err(e) = core
        .manager
        .create_with_id(id.clone(), ShareSource::Receive { dir })
    {
        log::error!("Empfang fehlgeschlagen: {e}");
        core.set_failed(&id, ShareError::Generic);
    }
    if open_session(app, &core, &id, SessionView::Receive) {
        log::info!("Empfang vom Handy gestartet");
    }
}

/// Starts a share for a text (clipboard, selection). Links get a direct QR
/// code, everything else goes through the landing page.
pub fn start_text_share(app: &AppHandle, text: &str) {
    let Some(core) = app_core::get(app) else {
        return;
    };
    let Some(id) = new_session_id(app) else {
        return;
    };
    match ShareSource::from_text(text) {
        Some(source) => {
            if let Err(e) = core.manager.create_with_id(id.clone(), source) {
                log::error!("Text-Freigabe fehlgeschlagen: {e}");
                core.set_failed(&id, ShareError::Generic);
            }
        }
        None if text.trim().is_empty() => core.set_failed(&id, ShareError::ClipboardEmpty),
        None => core.set_failed(&id, ShareError::TextTooLarge),
    }
    if open_session(app, &core, &id, SessionView::Share) {
        // Never log the text itself.
        log::info!("Neue Text-Freigabe");
    }
}

/// Shares the current clipboard text.
pub fn share_clipboard(app: &AppHandle) {
    use tauri_plugin_clipboard_manager::ClipboardExt;
    let text = app.clipboard().read_text().unwrap_or_else(|e| {
        // Empty or not text (e.g. an image): shown as "clipboard empty".
        log::info!("Zwischenablage ohne Text: {e}");
        String::new()
    });
    start_text_share(app, &text);
}
