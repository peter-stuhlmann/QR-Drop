//! Telling the user that QR Drop could not start.
//!
//! A GUI app has no console, so the error goes to the log and into a native
//! message box. The dialog plugin cannot help here: its dialogs run on the
//! event loop, which never starts when building the app failed.

const TITLE: &str = "QR Drop";
const TEXT_DE: &str = "QR Drop konnte nicht gestartet werden. Bitte starte den Computer neu und versuche es dann noch einmal.";
const TEXT_EN: &str =
    "QR Drop could not be started. Please restart your computer and then try again.";

/// Logs the error and shows a human message box.
pub fn report(error: &dyn std::fmt::Display) {
    log::error!("QR Drop konnte nicht starten: {error}");
    let text = if system_prefers_german() {
        TEXT_DE
    } else {
        TEXT_EN
    };
    show_message_box(text);
}

#[cfg(windows)]
mod native {
    use std::ffi::c_void;

    // user32 and kernel32 are linked by every Windows GUI app anyway; these
    // two declarations avoid a further dependency for this one dialog.
    #[link(name = "user32")]
    extern "system" {
        fn MessageBoxW(hwnd: *mut c_void, text: *const u16, caption: *const u16, kind: u32) -> i32;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetUserDefaultUILanguage() -> u16;
    }

    const MB_OK: u32 = 0x0000_0000;
    const MB_ICONERROR: u32 = 0x0000_0010;
    /// Primary language id of German (`LANG_GERMAN`).
    const LANG_GERMAN: u16 = 0x07;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn prefers_german() -> bool {
        // SAFETY: no arguments, only reads the user's UI language.
        let lang = unsafe { GetUserDefaultUILanguage() };
        lang & 0x03FF == LANG_GERMAN
    }

    pub fn message_box(title: &str, text: &str) {
        let (title, text) = (wide(title), wide(text));
        // SAFETY: both strings are NUL-terminated UTF-16 buffers that live
        // until the call returns; a null owner window is allowed.
        unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                text.as_ptr(),
                title.as_ptr(),
                MB_OK | MB_ICONERROR,
            );
        }
    }
}

#[cfg(windows)]
fn system_prefers_german() -> bool {
    native::prefers_german()
}

#[cfg(windows)]
fn show_message_box(text: &str) {
    native::message_box(TITLE, text);
}

#[cfg(target_os = "macos")]
fn system_prefers_german() -> bool {
    crate::integration::macos::system_prefers_german()
}

#[cfg(target_os = "macos")]
fn show_message_box(text: &str) {
    // Fixed texts without quotes, so they are safe inside the AppleScript
    // string literal.
    let script = format!("display alert \"{TITLE}\" message \"{text}\" as critical");
    if let Err(e) = std::process::Command::new("/usr/bin/osascript")
        .args(["-e", &script])
        .status()
    {
        log::warn!("Fehlermeldung nicht angezeigt: {e}");
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
fn system_prefers_german() -> bool {
    false
}

#[cfg(not(any(windows, target_os = "macos")))]
fn show_message_box(text: &str) {
    eprintln!("{TITLE}: {text}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_are_safe_for_applescript() {
        for text in [TITLE, TEXT_DE, TEXT_EN] {
            assert!(!text.contains('"') && !text.contains('\\'));
        }
    }
}
