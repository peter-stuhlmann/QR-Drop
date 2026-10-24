//! Small persisted settings (port, first-run flag) in the app config folder.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const FILE_NAME: &str = "settings.json";

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Last used server port, reused so firewall approvals stay valid.
    pub port: Option<u16>,
    /// Welcome window was shown (macOS first run).
    pub welcomed: bool,
}

fn file(dir: &Path) -> PathBuf {
    dir.join(FILE_NAME)
}

/// The saved settings; defaults if the file is missing or damaged.
pub fn load(dir: &Path) -> Settings {
    std::fs::read_to_string(file(dir))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Writes the settings atomically (temp file + rename), so a crash or a
/// full disk never leaves a half-written file behind.
pub fn save(dir: &Path, settings: &Settings) {
    let result = std::fs::create_dir_all(dir).and_then(|_| {
        let json = serde_json::to_string_pretty(settings).map_err(std::io::Error::other)?;
        let tmp = dir.join(format!("{FILE_NAME}.tmp"));
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, file(dir))
    });
    if let Err(e) = result {
        log::warn!("Einstellungen nicht gespeichert: {}", e.kind());
    }
}

/// Remembers the server port if it changed.
pub fn remember_port(dir: &Path, settings: &mut Settings, port: u16) {
    if settings.port != Some(port) {
        settings.port = Some(port);
        save(dir, settings);
    }
}

/// Remembers that the welcome window was confirmed.
pub fn mark_welcomed(dir: &Path) {
    let mut settings = load(dir);
    if !settings.welcomed {
        settings.welcomed = true;
        save(dir, &settings);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load(dir.path()), Settings::default());
        save(
            dir.path(),
            &Settings {
                port: Some(52817),
                welcomed: true,
            },
        );
        let s = load(dir.path());
        assert_eq!(s.port, Some(52817));
        assert!(s.welcomed);
        assert!(!dir.path().join("settings.json.tmp").exists());
        std::fs::write(file(dir.path()), "kaputt").unwrap();
        assert_eq!(load(dir.path()).port, None);
    }

    #[test]
    fn port_and_welcome_are_remembered() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = load(dir.path());
        remember_port(dir.path(), &mut s, 50000);
        mark_welcomed(dir.path());
        assert_eq!(
            load(dir.path()),
            Settings {
                port: Some(50000),
                welcomed: true,
            }
        );
    }
}
