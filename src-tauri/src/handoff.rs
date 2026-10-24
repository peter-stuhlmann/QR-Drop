//! Hand-off files of the macOS services (CLAUDE.md §6.2).
//!
//! A service can only start QR Drop with file paths, so "Text per QR-Code
//! senden" writes the selected text, and "Vom Handy empfangen" an empty
//! request file, into a private directory created by
//! `mktemp -d "$TMPDIR/qrdrop.XXXXXX"` (mode 0700) and opens that file with
//! the app. Only files that match exactly this layout are treated as
//! hand-offs -- and deleted after use; everything else is an ordinary file
//! to share. Other platforms have no such services: there every path is a
//! file.

use std::io::Read;
use std::path::{Path, PathBuf};

use crate::share::MAX_TEXT_BYTES;

/// Extension of a selected text handed over by the text service.
const TEXT_EXT: &str = "qrdroptext";
/// Extension of the (empty) request of the receive service.
const RECEIVE_EXT: &str = "qrdropreceive";
/// Name prefix of the private directory the workflows create.
const DIR_PREFIX: &str = "qrdrop.";

/// A recognised hand-off file.
#[derive(Debug, PartialEq, Eq)]
pub enum Handoff {
    Text(PathBuf),
    Receive(PathBuf),
}

/// Splits paths into ordinary files and hand-offs.
pub fn split(paths: &[PathBuf]) -> (Vec<PathBuf>, Vec<Handoff>) {
    split_in(paths, &temp_roots())
}

/// Directories the workflows create their private directory in (`$TMPDIR`,
/// or `/tmp` if it is unset), canonicalised.
#[cfg(target_os = "macos")]
fn temp_roots() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = [std::env::temp_dir(), PathBuf::from("/tmp")]
        .iter()
        .filter_map(|dir| dir.canonicalize().ok())
        .collect();
    roots.dedup();
    roots
}

#[cfg(not(target_os = "macos"))]
fn temp_roots() -> Vec<PathBuf> {
    Vec::new()
}

fn split_in(paths: &[PathBuf], roots: &[PathBuf]) -> (Vec<PathBuf>, Vec<Handoff>) {
    let mut files = Vec::new();
    let mut handoffs = Vec::new();
    for path in paths {
        match classify(path, roots) {
            Some(handoff) => handoffs.push(handoff),
            None => files.push(path.clone()),
        }
    }
    (files, handoffs)
}

/// `<root>/qrdrop.XXXXXX/<name>.qrdroptext|.qrdropreceive`, with a real
/// directory and a regular file (no symlinks).
fn classify(path: &Path, roots: &[PathBuf]) -> Option<Handoff> {
    let make: fn(PathBuf) -> Handoff = match path.extension()?.to_str()? {
        TEXT_EXT => Handoff::Text,
        RECEIVE_EXT => Handoff::Receive,
        _ => return None,
    };
    let dir = path.parent()?;
    if !dir.file_name()?.to_str()?.starts_with(DIR_PREFIX) {
        return None;
    }
    let root = dir.parent()?.canonicalize().ok()?;
    if !roots.contains(&root) {
        return None;
    }
    let dir_is_real = std::fs::symlink_metadata(dir).is_ok_and(|m| m.file_type().is_dir());
    let file_is_real = std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_file());
    (dir_is_real && file_is_real).then(|| make(path.to_path_buf()))
}

/// Reads a handed-over text. Reads one byte past the limit so an oversized
/// text still reaches the share and shows "text too large" instead of
/// nothing.
pub fn read_text(path: &Path) -> Option<String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|f| f.take(MAX_TEXT_BYTES as u64 + 1).read_to_end(&mut bytes))
        .ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Deletes a hand-off file and its private directory. The text may be a
/// password, so it must not stay on disk.
pub fn discard(path: &Path) {
    if let Err(e) = std::fs::remove_file(path) {
        log::warn!("Übergabedatei nicht gelöscht: {}", e.kind());
    }
    // `remove_dir` only removes an empty directory: never more than the
    // workflow created.
    if let Some(dir) = path.parent() {
        if let Err(e) = std::fs::remove_dir(dir) {
            log::warn!("Übergabeordner nicht gelöscht: {}", e.kind());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A temp root plus a private directory like the workflows create.
    fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let canonical = root.path().canonicalize().unwrap();
        let private = root.path().join("qrdrop.Ab12Cd");
        std::fs::create_dir(&private).unwrap();
        (root, canonical, private)
    }

    #[test]
    fn recognises_only_the_workflow_layout() {
        let (root, canonical, private) = fixture();
        let roots = [canonical];
        let text = private.join("selection.qrdroptext");
        let receive = private.join("request.qrdropreceive");
        std::fs::write(&text, "Hallo").unwrap();
        std::fs::write(&receive, "").unwrap();
        assert_eq!(classify(&text, &roots), Some(Handoff::Text(text.clone())));
        assert_eq!(
            classify(&receive, &roots),
            Some(Handoff::Receive(receive.clone()))
        );

        // Right extension, wrong place: an ordinary file.
        let loose = root.path().join("loose.qrdroptext");
        std::fs::write(&loose, "x").unwrap();
        assert_eq!(classify(&loose, &roots), None);
        let other_dir = root.path().join("other");
        std::fs::create_dir(&other_dir).unwrap();
        let foreign = other_dir.join("selection.qrdroptext");
        std::fs::write(&foreign, "x").unwrap();
        assert_eq!(classify(&foreign, &roots), None);
        // Right place, wrong extension.
        let photo = private.join("photo.jpg");
        std::fs::write(&photo, "x").unwrap();
        assert_eq!(classify(&photo, &roots), None);
        // Outside every temp root (and without any root: other platforms).
        assert_eq!(classify(&text, &[]), None);
        // Missing file.
        assert_eq!(classify(&private.join("gone.qrdroptext"), &roots), None);

        let (files, handoffs) = split_in(&[photo.clone(), text.clone()], &roots);
        assert_eq!(files, vec![photo]);
        assert_eq!(handoffs, vec![Handoff::Text(text)]);
    }

    #[test]
    fn text_is_read_and_removed_with_its_directory() {
        let (_root, _canonical, private) = fixture();
        let path = private.join("selection.qrdroptext");
        std::fs::write(&path, "Hallo Handy 👋").unwrap();
        assert_eq!(read_text(&path).as_deref(), Some("Hallo Handy 👋"));
        discard(&path);
        assert!(!path.exists());
        assert!(!private.exists());
    }

    #[test]
    fn discard_never_removes_other_files() {
        let (_root, _canonical, private) = fixture();
        let path = private.join("selection.qrdroptext");
        std::fs::write(&path, "x").unwrap();
        std::fs::write(private.join("unrelated"), "keep").unwrap();
        discard(&path);
        assert!(!path.exists());
        assert!(private.join("unrelated").exists());
    }

    #[test]
    fn oversized_text_still_reports_too_large() {
        let (_root, _canonical, private) = fixture();
        let path = private.join("big.qrdroptext");
        std::fs::write(&path, "x".repeat(MAX_TEXT_BYTES + 10)).unwrap();
        let text = read_text(&path).unwrap();
        assert_eq!(text.len(), MAX_TEXT_BYTES + 1);
        assert!(crate::share::ShareSource::from_text(&text).is_none());
    }
}
