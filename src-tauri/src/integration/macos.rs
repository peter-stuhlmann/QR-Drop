//! macOS: installs the bundled Automator services into ~/Library/Services --
//! the Finder Quick Action for files, the text service and "Vom Handy
//! empfangen" -- named in the system language.

use std::io;
use std::path::PathBuf;
use std::process::Command;

use tauri::{AppHandle, Manager};

use super::services;

fn services_dir() -> io::Result<PathBuf> {
    std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join("Library/Services"))
        .ok_or_else(|| io::Error::other("HOME not set"))
}

/// The UI follows the system language; so do the service titles. Unknown
/// counts as German, like the UI default.
pub fn system_prefers_german() -> bool {
    Command::new("defaults")
        .args(["read", "-g", "AppleLanguages"])
        .output()
        .ok()
        .and_then(|out| services::prefers_german(&String::from_utf8_lossy(&out.stdout)))
        .unwrap_or(true)
}

/// Installs all workflows and refreshes the Services cache if anything
/// changed. Returns true if at least one was (re)installed. Blocking: runs
/// on a background thread (see [`super::on_startup`]).
pub fn ensure_quick_action(app: &AppHandle) -> io::Result<bool> {
    let resources = app
        .path()
        .resource_dir()
        .map_err(io::Error::other)?
        .join("macos");
    let changed = services::install(&resources, &services_dir()?, system_prefers_german())?;
    if changed {
        // Make Finder and other apps pick up the services without a logout.
        // Waiting is fine on this background thread and reaps the child.
        match Command::new("/System/Library/CoreServices/pbs")
            .arg("-update")
            .status()
        {
            Ok(status) if status.success() => {}
            Ok(status) => log::warn!("Dienste-Cache nicht aktualisiert: {status}"),
            Err(e) => log::warn!("Dienste-Cache nicht aktualisiert: {e}"),
        }
        log::info!("Dienste installiert");
    }
    Ok(changed)
}
