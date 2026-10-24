//! Installs the bundled Automator services (macOS) into the user's Services
//! folder, named in the system language. Plain file operations, so this is
//! tested on every platform even though only macOS uses it.

use std::io;
use std::path::Path;

/// One bundled workflow and its menu title per language.
pub struct Workflow {
    /// Folder name in the app's resources.
    pub resource: &'static str,
    pub de: &'static str,
    pub en: &'static str,
}

pub const WORKFLOWS: [Workflow; 3] = [
    Workflow {
        resource: "Per QR-Code senden.workflow",
        de: "Per QR-Code senden",
        en: "Send via QR code",
    },
    Workflow {
        resource: "Text per QR-Code senden.workflow",
        de: "Text per QR-Code senden",
        en: "Send text via QR code",
    },
    Workflow {
        resource: "Vom Handy empfangen.workflow",
        de: "Vom Handy empfangen",
        en: "Receive from phone",
    },
];

/// Reads the output of `defaults read -g AppleLanguages` (a plist array like
/// `( "de-DE", "en-US" )`): is the preferred language German? `None` if the
/// output holds no language at all.
pub fn prefers_german(apple_languages: &str) -> Option<bool> {
    apple_languages
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
        .find(|token| !token.is_empty())
        .map(|first| first.to_ascii_lowercase().starts_with("de"))
}

fn copy_dir(src: &Path, dst: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let target = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn same_content(path: &Path, expected: &[u8]) -> bool {
    std::fs::read(path).is_ok_and(|content| content == expected)
}

/// Copies one workflow under its name in the chosen language (if missing or
/// outdated) and removes the variant in the other language. Returns true if
/// anything changed.
fn install_one(resources: &Path, services: &Path, wf: &Workflow, german: bool) -> io::Result<bool> {
    let src = resources.join(wf.resource);
    let (name, other) = if german {
        (wf.de, wf.en)
    } else {
        (wf.en, wf.de)
    };
    let info = std::fs::read_to_string(src.join("Contents/Info.plist"))?;
    // Bundle name and menu title are the German title in the resources.
    let info = if german {
        info
    } else {
        info.replace(
            &format!("<string>{}</string>", wf.de),
            &format!("<string>{}</string>", wf.en),
        )
    };
    let document = std::fs::read(src.join("Contents/document.wflow"))?;

    let mut changed = false;
    let stale = services.join(format!("{other}.workflow"));
    if stale.exists() {
        std::fs::remove_dir_all(&stale)?;
        changed = true;
    }
    let dst = services.join(format!("{name}.workflow"));
    if same_content(&dst.join("Contents/Info.plist"), info.as_bytes())
        && same_content(&dst.join("Contents/document.wflow"), &document)
    {
        return Ok(changed);
    }
    if dst.exists() {
        std::fs::remove_dir_all(&dst)?;
    }
    copy_dir(&src, &dst)?;
    std::fs::write(dst.join("Contents/Info.plist"), info)?;
    Ok(true)
}

/// Installs all workflows. Returns true if anything changed (then the
/// Services cache needs a refresh).
pub fn install(resources: &Path, services: &Path, german: bool) -> io::Result<bool> {
    std::fs::create_dir_all(services)?;
    let mut changed = false;
    for wf in &WORKFLOWS {
        changed |= install_one(resources, services, wf, german)?;
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_preferred_language() {
        let german = "(\n    \"de-DE\",\n    \"en-US\"\n)\n";
        assert_eq!(prefers_german(german), Some(true));
        assert_eq!(
            prefers_german("(\n    \"en-GB\",\n    \"de\"\n)"),
            Some(false)
        );
        assert_eq!(prefers_german("(\n    de\n)"), Some(true));
        assert_eq!(prefers_german(""), None);
    }

    fn fake_resources(dir: &Path) {
        for wf in &WORKFLOWS {
            let contents = dir.join(wf.resource).join("Contents");
            std::fs::create_dir_all(&contents).unwrap();
            let info = format!(
                "<dict><key>CFBundleName</key><string>{0}</string><key>default</key><string>{0}</string></dict>",
                wf.de
            );
            std::fs::write(contents.join("Info.plist"), info).unwrap();
            std::fs::write(contents.join("document.wflow"), wf.resource).unwrap();
        }
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn installs_in_the_system_language_and_switches_cleanly() {
        let res = tempfile::tempdir().unwrap();
        let services = tempfile::tempdir().unwrap();
        fake_resources(res.path());

        assert!(install(res.path(), services.path(), false).unwrap());
        assert_eq!(
            names(services.path()),
            [
                "Receive from phone.workflow",
                "Send text via QR code.workflow",
                "Send via QR code.workflow"
            ]
        );
        let info = std::fs::read_to_string(
            services
                .path()
                .join("Send via QR code.workflow/Contents/Info.plist"),
        )
        .unwrap();
        assert!(info.contains("<string>Send via QR code</string>"));
        assert!(!info.contains("Per QR-Code"));
        // Nothing to do on the next start.
        assert!(!install(res.path(), services.path(), false).unwrap());

        // The system language changed to German.
        assert!(install(res.path(), services.path(), true).unwrap());
        assert_eq!(
            names(services.path()),
            [
                "Per QR-Code senden.workflow",
                "Text per QR-Code senden.workflow",
                "Vom Handy empfangen.workflow"
            ]
        );
    }
}
