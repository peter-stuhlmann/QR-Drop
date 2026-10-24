//! Windows: the NSIS installer registers the context menu (`hooks.nsh`).
//! The installed release build repairs a missing or outdated entry, e.g.
//! after a registry cleaner ran. Blocking: runs on a background thread (see
//! [`super::on_startup`]).
//!
//! Uses `reg.exe` (by full path) instead of the registry API to need no
//! further dependency: three short queries per start, writes only when
//! something is broken.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// Written by the Tauri NSIS template (`UNINSTKEY`).
const UNINSTALL_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\QR Drop";
/// Written by the Tauri NSIS template (`MANUPRODUCTKEY`, manufacturer from
/// the bundle identifier): holds the language chosen in the installer.
const PRODUCT_KEY: &str = r"HKCU\Software\qrdrop\QR Drop";
const INSTALLER_LANGUAGE: &str = "Installer Language";
/// LANGID of German, as NSIS reports it in `$LANGUAGE`.
const LANG_GERMAN: &str = "1031";
/// Context-menu verbs for files and folders (same as `hooks.nsh`).
const MENU_KEYS: [&str; 2] = [
    r"HKCU\Software\Classes\*\shell\QRDrop",
    r"HKCU\Software\Classes\Directory\shell\QRDrop",
];
const MULTI_SELECT_MODEL: &str = "Player";

fn reg_exe() -> PathBuf {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
    Path::new(&root).join("System32").join("reg.exe")
}

/// Runs `reg.exe`; stdout on success.
fn reg(args: &[&str]) -> Option<String> {
    let out = Command::new(reg_exe())
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// One key of a `reg query` listing with its string values.
#[derive(Debug, Default, PartialEq, Eq)]
struct RegKey {
    path: String,
    /// `(name, data)`; the default value has an empty name.
    values: Vec<(String, String)>,
}

impl RegKey {
    fn value(&self, name: &str) -> Option<&str> {
        self.values
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, data)| data.as_str())
    }
}

/// Parses the output of `reg query <key> [/v <name>] [/s]`: key lines start
/// with `HKEY_`, value lines are `    <name>    REG_<type>    <data>`. The
/// default value's name is localised ("(Default)", "(Standard)" ...), so any
/// name in parentheses counts as the default value.
fn parse_reg_query(output: &str) -> Vec<RegKey> {
    let mut keys: Vec<RegKey> = Vec::new();
    for line in output.lines() {
        if line.starts_with("HKEY_") {
            keys.push(RegKey {
                path: line.trim().to_string(),
                values: Vec::new(),
            });
            continue;
        }
        let mut parts = line.trim_start().splitn(3, "    ");
        let (Some(name), Some(kind), data) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        if !kind.starts_with("REG_") {
            continue;
        }
        let name = if name.starts_with('(') && name.ends_with(')') {
            ""
        } else {
            name
        };
        if let Some(key) = keys.last_mut() {
            key.values.push((
                name.to_string(),
                data.unwrap_or_default().trim().to_string(),
            ));
        }
    }
    keys
}

/// `reg query` prints the full hive name, our constants use the short one.
fn full_key_path(key: &str) -> String {
    match key.strip_prefix(r"HKCU\") {
        Some(rest) => format!(r"HKEY_CURRENT_USER\{rest}"),
        None => key.to_string(),
    }
}

/// Windows paths compare case-insensitively.
fn same_path(a: &str, b: &str) -> bool {
    a.trim_matches('"').to_lowercase() == b.trim_matches('"').to_lowercase()
}

/// What a menu key must contain to start this executable.
struct ExpectedMenu {
    command: String,
    icon: String,
}

impl ExpectedMenu {
    fn for_exe(exe: &str) -> Self {
        Self {
            command: format!("\"{exe}\" \"%1\""),
            icon: format!("\"{exe}\",0"),
        }
    }

    /// Whether the listing of `key` (queried with `/s`) is complete and
    /// points at this executable.
    fn is_satisfied_by(&self, key: &str, listing: &[RegKey]) -> bool {
        let key = full_key_path(key);
        let find = |path: &str| listing.iter().find(|k| k.path.eq_ignore_ascii_case(path));
        let command_key = format!(r"{key}\command");
        let (Some(verb), Some(command)) = (find(&key), find(&command_key)) else {
            return false;
        };
        verb.value("").is_some_and(|label| !label.is_empty())
            && verb.value("Icon").is_some_and(|i| same_path(i, &self.icon))
            && verb
                .value("MultiSelectModel")
                .is_some_and(|m| m.eq_ignore_ascii_case(MULTI_SELECT_MODEL))
            && command
                .value("")
                .is_some_and(|c| same_path(c, &self.command))
    }
}

/// The menu label in the language of the installer (same rule as
/// `hooks.nsh`: German for LANGID 1031, English otherwise).
fn menu_label() -> &'static str {
    let language = reg(&["query", PRODUCT_KEY, "/v", INSTALLER_LANGUAGE])
        .map(|out| parse_reg_query(&out))
        .and_then(|keys| {
            keys.into_iter()
                .find_map(|k| k.value(INSTALLER_LANGUAGE).map(str::to_string))
        });
    if language.as_deref() == Some(LANG_GERMAN) {
        "Per QR-Code senden"
    } else {
        "Send via QR code"
    }
}

/// Whether this executable is the copy the installer put in place (never
/// point the menu at a build that just runs from some folder).
fn is_installed_copy(exe: &Path) -> bool {
    let Some(dir) = exe.parent().and_then(Path::to_str) else {
        return false;
    };
    reg(&["query", UNINSTALL_KEY, "/v", "InstallLocation"])
        .map(|out| parse_reg_query(&out))
        .and_then(|keys| {
            keys.iter()
                .find_map(|k| k.value("InstallLocation").map(|l| same_path(l, dir)))
        })
        .unwrap_or(false)
}

fn repair(key: &str, expected: &ExpectedMenu, label: &str) {
    log::info!("Kontextmenü-Eintrag wird repariert");
    let command_key = format!(r"{key}\command");
    let writes: [&[&str]; 4] = [
        &["add", key, "/ve", "/d", label, "/f"],
        &["add", key, "/v", "Icon", "/d", &expected.icon, "/f"],
        &[
            "add",
            key,
            "/v",
            "MultiSelectModel",
            "/d",
            MULTI_SELECT_MODEL,
            "/f",
        ],
        &["add", &command_key, "/ve", "/d", &expected.command, "/f"],
    ];
    for args in writes {
        if reg(args).is_none() {
            log::warn!("Kontextmenü-Eintrag nicht geschrieben");
            return;
        }
    }
}

pub fn ensure_context_menu() {
    if cfg!(debug_assertions) {
        return; // never point the menu at a dev build
    }
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let Some(exe_str) = exe.to_str() else {
        return;
    };
    // `reg query` prints in the console code page, so a path with non-ASCII
    // characters cannot be compared reliably; the installer wrote it
    // correctly, and a false mismatch would rewrite the menu on every start.
    if !exe_str.is_ascii() || !is_installed_copy(&exe) {
        return;
    }
    let expected = ExpectedMenu::for_exe(exe_str);
    let mut label = None;
    for key in MENU_KEYS {
        let listing = reg(&["query", key, "/s"])
            .map(|out| parse_reg_query(&out))
            .unwrap_or_default();
        if !expected.is_satisfied_by(key, &listing) {
            repair(key, &expected, label.get_or_insert_with(menu_label));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXE: &str = r"C:\Users\info\AppData\Local\QR Drop\QR Drop.exe";
    const KEY: &str = r"HKEY_CURRENT_USER\Software\Classes\*\shell\QRDrop";

    fn listing(icon: &str, command: &str) -> String {
        format!(
            "\r\n{KEY}\r\n    (Standard)    REG_SZ    Per QR-Code senden\r\n    Icon    REG_SZ    {icon}\r\n    MultiSelectModel    REG_SZ    Player\r\n\r\n{KEY}\\command\r\n    (Standard)    REG_SZ    {command}\r\n\r\n"
        )
    }

    #[test]
    fn parses_keys_and_values() {
        let keys = parse_reg_query(&listing("\"x\",0", "\"x\" \"%1\""));
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0].path, KEY);
        assert_eq!(keys[0].value(""), Some("Per QR-Code senden"));
        assert_eq!(keys[0].value("icon"), Some("\"x\",0"));
        assert_eq!(keys[1].value(""), Some("\"x\" \"%1\""));

        let language = "\r\nHKEY_CURRENT_USER\\Software\\qrdrop\\QR Drop\r\n    Installer Language    REG_SZ    1031\r\n";
        let keys = parse_reg_query(language);
        assert_eq!(keys[0].value(INSTALLER_LANGUAGE), Some("1031"));
        assert!(parse_reg_query("").is_empty());
    }

    #[test]
    fn checks_every_value_case_insensitively() {
        // `reg query` reports the full hive name; ours use the HKCU prefix.
        let key = MENU_KEYS[0];
        let expected = ExpectedMenu::for_exe(EXE);
        let ok = listing(&expected.icon, &expected.command);
        assert!(expected.is_satisfied_by(key, &parse_reg_query(&ok)));
        let other_case = listing(
            &expected.icon.to_uppercase(),
            &expected.command.to_lowercase(),
        );
        assert!(expected.is_satisfied_by(key, &parse_reg_query(&other_case)));

        let moved = ExpectedMenu::for_exe(r"D:\Apps\QR Drop.exe");
        let stale = listing(&moved.icon, &moved.command);
        assert!(!expected.is_satisfied_by(key, &parse_reg_query(&stale)));
        let stale_icon = listing(&moved.icon, &expected.command);
        assert!(!expected.is_satisfied_by(key, &parse_reg_query(&stale_icon)));
        let no_multi = ok.replace("    MultiSelectModel    REG_SZ    Player\r\n", "");
        assert!(!expected.is_satisfied_by(key, &parse_reg_query(&no_multi)));
        assert!(!expected.is_satisfied_by(key, &[]));
    }

    #[test]
    fn paths_compare_without_quotes_and_case() {
        assert!(same_path(
            r#""C:\Users\Info\AppData\Local\QR Drop""#,
            r"c:\users\info\appdata\local\qr drop"
        ));
        assert!(!same_path(r"C:\A", r"C:\B"));
    }
}
