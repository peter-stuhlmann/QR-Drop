//! Tauri commands called by the frontend (`src/lib/tauri.ts`).
//!
//! All commands are async so they never run on the main thread. Which
//! window may call which command is defined in `capabilities/` (the list of
//! commands in `build.rs` must match `generate_handler!` in `lib.rs`).

use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Manager, State, WebviewWindow};
use tauri_plugin_dialog::{DialogExt, FilePath};

use crate::app_core::{AppCore, ShareInfo};
use crate::args::paths_from_args;
use crate::sessions;
use crate::settings;
use crate::share::ShareSource;
use crate::ui;
use crate::update::{self, FoundUpdate, InstallError, UpdateInfo};

/// Errors cross the IPC boundary as plain codes (never paths or system
/// messages); the UI maps them to human texts.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CommandError {
    /// The session does not exist (any more).
    UnknownShare,
    InvalidIp,
    /// None of the given paths exists.
    NotFound,
    /// The session could not be started (window or randomness missing).
    StartFailed,
    /// A transfer is running; an update would cut it off.
    Busy,
    NoUpdate,
    UpdateFailed,
    /// Dev test run of the updater finished without installing.
    #[cfg(debug_assertions)]
    DryRun,
    RevealFailed,
    DialogFailed,
    WindowFailed,
}

type CmdResult<T> = Result<T, CommandError>;

impl From<InstallError> for CommandError {
    fn from(e: InstallError) -> Self {
        match e {
            InstallError::NoUpdate => Self::NoUpdate,
            InstallError::Failed => Self::UpdateFailed,
            #[cfg(debug_assertions)]
            InstallError::DryRun => Self::DryRun,
        }
    }
}

fn share_info(core: &AppCore, id: &str) -> CmdResult<ShareInfo> {
    core.info(id).ok_or(CommandError::UnknownShare)
}

// --- Session windows ---------------------------------------------------------

#[tauri::command]
pub async fn get_share(core: State<'_, Arc<AppCore>>, id: String) -> CmdResult<ShareInfo> {
    if let Some(latency) = core.take_request_latency() {
        log::info!("Fenster bereit nach {} ms", latency.as_millis());
    }
    share_info(&core, &id)
}

/// "Anderes Netzwerk?": the QR code with another LAN address.
#[tauri::command]
pub async fn select_network(
    core: State<'_, Arc<AppCore>>,
    id: String,
    ip: String,
) -> CmdResult<ShareInfo> {
    let ip: Ipv4Addr = ip.parse().map_err(|_| CommandError::InvalidIp)?;
    core.select_ip(&id, ip);
    share_info(&core, &id)
}

/// A new token and a fresh expiry for an expired share.
#[tauri::command]
pub async fn renew_share(core: State<'_, Arc<AppCore>>, id: String) -> CmdResult<ShareInfo> {
    core.manager.renew(&id).ok_or(CommandError::UnknownShare)?;
    share_info(&core, &id)
}

#[tauri::command]
pub async fn cancel_share(core: State<'_, Arc<AppCore>>, id: String) -> CmdResult<()> {
    core.forget(&id);
    Ok(())
}

/// "Ordner öffnen": shows the received files in Explorer/Finder (or the
/// target folder if nothing arrived yet). Paths never come from the UI.
#[tauri::command]
pub async fn reveal_received(core: State<'_, Arc<AppCore>>, id: String) -> CmdResult<()> {
    let files: Vec<PathBuf> = core
        .received_paths(&id)
        .into_iter()
        .filter(|p| p.exists())
        .collect();
    let result = if files.is_empty() {
        let share = core.manager.get(&id).ok_or(CommandError::UnknownShare)?;
        let ShareSource::Receive { dir } = &*share.source else {
            return Err(CommandError::UnknownShare);
        };
        tauri_plugin_opener::open_path(dir, None::<&str>)
    } else {
        tauri_plugin_opener::reveal_items_in_dir(&files)
    };
    result.map_err(|e| {
        log::warn!("Ordner nicht geöffnet: {e}");
        CommandError::RevealFailed
    })
}

// --- Drop zone ---------------------------------------------------------------

/// Paths dropped onto the drop zone.
#[tauri::command]
pub async fn share_paths(app: AppHandle, paths: Vec<String>) -> CmdResult<()> {
    let paths = paths_from_args(paths, None);
    if paths.is_empty() {
        return Err(CommandError::NotFound);
    }
    sessions::start_share(&app, paths)
        .map(drop)
        .ok_or(CommandError::StartFailed)
}

/// "Zwischenablage senden" button and Ctrl+V / Cmd+V.
#[tauri::command]
pub async fn share_clipboard(app: AppHandle) -> CmdResult<()> {
    sessions::share_clipboard(&app);
    Ok(())
}

/// "Vom Handy empfangen".
#[tauri::command]
pub async fn receive_from_phone(app: AppHandle) -> CmdResult<()> {
    sessions::start_receive(&app);
    Ok(())
}

/// "Datei auswählen …": the dialog runs in Rust, so the frontend needs no
/// file-system permissions. Returns whether something was picked.
#[tauri::command]
pub async fn pick_files(app: AppHandle, window: WebviewWindow, folder: bool) -> CmdResult<bool> {
    let dialog = app.dialog().file().set_parent(&window);
    let (tx, rx) = tokio::sync::oneshot::channel::<Vec<FilePath>>();
    let send = move |picked: Vec<FilePath>| {
        // The receiver only goes away if the command was dropped.
        let _ = tx.send(picked);
    };
    if folder {
        dialog.pick_folder(move |picked| send(picked.into_iter().collect()));
    } else {
        dialog.pick_files(move |picked| send(picked.unwrap_or_default()));
    }
    let picked = rx.await.map_err(|_| CommandError::DialogFailed)?;
    let paths: Vec<PathBuf> = picked
        .into_iter()
        .filter_map(|p| p.into_path().ok())
        .collect();
    if paths.is_empty() {
        return Ok(false);
    }
    sessions::start_share(&app, paths)
        .map(|_| true)
        .ok_or(CommandError::StartFailed)
}

/// Looks for a newer version (GitHub releases).
#[tauri::command]
pub async fn check_update(
    app: AppHandle,
    found: State<'_, FoundUpdate>,
) -> CmdResult<Option<UpdateInfo>> {
    Ok(update::check(&app, &found).await)
}

/// Installs the update and restarts; returns only on errors.
#[tauri::command]
pub async fn install_update(
    app: AppHandle,
    core: State<'_, Arc<AppCore>>,
    found: State<'_, FoundUpdate>,
) -> CmdResult<()> {
    // Never cut off a running transfer.
    if core.has_sessions() {
        return Err(CommandError::Busy);
    }
    update::install(&app, &found).await.map_err(Into::into)
}

// --- Welcome -----------------------------------------------------------------

/// Welcome window was confirmed: remember it and continue with the drop zone.
#[tauri::command]
pub async fn finish_welcome(app: AppHandle) -> CmdResult<()> {
    match app.path().app_config_dir() {
        Ok(dir) => settings::mark_welcomed(&dir),
        Err(e) => log::warn!("Einstellungsordner unbekannt: {e}"),
    }
    ui::open_dropzone(&app).map_err(|e| {
        log::error!("Fenster konnte nicht geöffnet werden: {e}");
        CommandError::WindowFailed
    })?;
    ui::close(&app, ui::WELCOME);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_are_plain_codes() {
        let codes = [
            (CommandError::UnknownShare, "unknown_share"),
            (CommandError::InvalidIp, "invalid_ip"),
            (CommandError::NotFound, "not_found"),
            (CommandError::StartFailed, "start_failed"),
            (CommandError::Busy, "busy"),
            (CommandError::NoUpdate, "no_update"),
            (CommandError::UpdateFailed, "update_failed"),
            (CommandError::RevealFailed, "reveal_failed"),
            (CommandError::DialogFailed, "dialog_failed"),
            (CommandError::WindowFailed, "window_failed"),
        ];
        for (error, code) in codes {
            assert_eq!(serde_json::to_value(error).unwrap(), code);
        }
    }

    #[test]
    fn install_errors_map_to_codes() {
        assert_eq!(
            CommandError::from(InstallError::NoUpdate),
            CommandError::NoUpdate
        );
        assert_eq!(
            CommandError::from(InstallError::Failed),
            CommandError::UpdateFailed
        );
    }
}
