//! Self-update from GitHub releases (tauri-plugin-updater). QR Drop itself
//! never needs the internet; this is the only outgoing request.

use std::sync::{Mutex, MutexGuard};

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::{Update, Updater, UpdaterExt};

use crate::ui;

/// A newer version, as shown in the drop zone.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
}

/// Why an update could not be installed.
#[derive(Debug, PartialEq, Eq)]
pub enum InstallError {
    /// Updater unavailable or no newer version.
    NoUpdate,
    /// Download, signature check or installation failed.
    Failed,
    /// Dev test run (`QRDROP_UPDATE_DRY_RUN`): verified, not installed.
    #[cfg(debug_assertions)]
    DryRun,
}

/// The update found by the last check, so installing does not ask the
/// server a second time.
#[derive(Default)]
pub struct FoundUpdate(Mutex<Option<Update>>);

impl FoundUpdate {
    fn lock(&self) -> MutexGuard<'_, Option<Update>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// The updater for the GitHub release address in `tauri.conf.json`. Dev
/// builds can point it at a local test server (`QRDROP_UPDATE_URL`) to try
/// updates without publishing a release.
fn updater(app: &AppHandle) -> Option<Updater> {
    #[cfg(debug_assertions)]
    if let Some(url) = std::env::var("QRDROP_UPDATE_URL")
        .ok()
        .and_then(|u| u.parse().ok())
    {
        return app
            .updater_builder()
            .endpoints(vec![url])
            .and_then(|builder| builder.build())
            .map_err(|e| log::warn!("Test-Updater nicht verfügbar: {e}"))
            .ok();
    }
    app.updater()
        .map_err(|e| log::warn!("Updater nicht verfügbar: {e}"))
        .ok()
}

/// Looks for a newer version. `None` when there is none or the computer is
/// offline.
pub async fn check(app: &AppHandle, found: &FoundUpdate) -> Option<UpdateInfo> {
    let update = match updater(app)?.check().await {
        Ok(update) => update?,
        Err(e) => {
            log::info!("Keine Update-Prüfung möglich: {e}");
            return None;
        }
    };
    log::info!("Update verfügbar: {}", update.version);
    let info = UpdateInfo {
        version: update.version.clone(),
    };
    *found.lock() = Some(update);
    Some(info)
}

/// Downloads, verifies (signature) and installs the update found by
/// [`check`] (or checks again), then restarts. Progress goes to the drop
/// zone as `update://progress` with `[downloaded, total]`.
pub async fn install(app: &AppHandle, found: &FoundUpdate) -> Result<(), InstallError> {
    let cached = found.lock().take();
    let update = match cached {
        Some(update) => update,
        None => {
            let updater = updater(app).ok_or(InstallError::NoUpdate)?;
            updater
                .check()
                .await
                .map_err(|e| {
                    log::warn!("Update-Prüfung fehlgeschlagen: {e}");
                    InstallError::Failed
                })?
                .ok_or(InstallError::NoUpdate)?
        }
    };
    #[cfg(debug_assertions)]
    if std::env::var_os("QRDROP_UPDATE_DRY_RUN").is_some() {
        return dry_run(update).await;
    }
    let handle = app.clone();
    let mut downloaded: u64 = 0;
    update
        .download_and_install(
            move |chunk, total| {
                downloaded += chunk as u64;
                if let Err(e) =
                    handle.emit_to(ui::DROPZONE, "update://progress", (downloaded, total))
                {
                    log::debug!("Fortschritt nicht zugestellt: {e}");
                }
            },
            || log::info!("Update geladen, wird installiert"),
        )
        .await
        .map_err(|e| {
            // e.g. an invalid signature: nothing gets installed.
            log::warn!("Update fehlgeschlagen: {e}");
            InstallError::Failed
        })?;
    // Windows has already handed over to the installer (which restarts us).
    app.restart();
}

/// Dev test run: download and verify the signature, but do not install.
#[cfg(debug_assertions)]
async fn dry_run(update: Update) -> Result<(), InstallError> {
    let bytes = update.download(|_, _| {}, || {}).await.map_err(|e| {
        log::warn!("Testlauf: Update abgelehnt: {e}");
        InstallError::Failed
    })?;
    log::info!(
        "Testlauf: Update {} geladen ({} Bytes), Signatur gültig",
        update.version,
        bytes.len()
    );
    Err(InstallError::DryRun)
}
