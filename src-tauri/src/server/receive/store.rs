//! Files on disk: hidden part files while an upload runs, and their final,
//! race-free placement in the target folder.

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::share::new_id;

const PART_PREFIX: &str = ".qrdrop-";
const PART_SUFFIX: &str = ".part";

/// Part files older than this are leftovers of a crash: a receive session
/// lasts an hour at most.
const STALE_AFTER: Duration = Duration::from_secs(24 * 60 * 60);

/// Numbered names (`name (2)` …) tried before a random suffix is used.
const MAX_NUMBERED: u32 = 999;

/// Random-suffix names tried after the numbered ones.
const RANDOM_ATTEMPTS: u32 = 8;

/// A new, unused part file name in `dir`.
pub(super) fn new_part_path(dir: &Path) -> io::Result<PathBuf> {
    let id = new_id().map_err(|e| io::Error::other(e.to_string()))?;
    Ok(dir.join(format!("{PART_PREFIX}{id}{PART_SUFFIX}")))
}

/// Opens a part file for writing, creating it if needed. Never in append
/// mode: the caller positions explicitly. A leading dot hides the file on
/// macOS; on Windows it gets the hidden attribute.
pub(super) async fn open_part(path: &Path, create_new: bool) -> io::Result<tokio::fs::File> {
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true);
    if create_new {
        options.create_new(true);
    } else {
        options.create(true);
    }
    #[cfg(windows)]
    options.attributes(windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_HIDDEN);
    options.open(path).await
}

/// Moves a complete part file to `dir/name`, or `name (2)`, `name (3)` … if
/// that is taken. Never replaces anything: each candidate is first reserved
/// with an exclusive create -- atomic, and it also refuses dangling
/// symlinks -- and the part is then renamed over that own, empty
/// placeholder. Blocking.
pub(super) fn place_file(part: &Path, dir: &Path, name: &str) -> io::Result<PathBuf> {
    mark_as_downloaded(part);
    reveal(part)?;
    for candidate in candidate_names(name) {
        let target = dir.join(&candidate);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
        {
            Ok(placeholder) => {
                drop(placeholder);
                return match std::fs::rename(part, &target) {
                    Ok(()) => Ok(target),
                    Err(e) => {
                        let _ = std::fs::remove_file(&target);
                        Err(e)
                    }
                };
            }
            // Taken (Windows reports an existing folder as "access denied").
            Err(_) if target.symlink_metadata().is_ok() => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "no free file name",
    ))
}

/// `name`, `name (2)` … `name (999)`, then a few random suffixes.
fn candidate_names(name: &str) -> impl Iterator<Item = String> + '_ {
    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem, format!(".{ext}")),
        _ => (name, String::new()),
    };
    let numbered = (2..=MAX_NUMBERED).map(move |n| n.to_string());
    let random = (0..RANDOM_ATTEMPTS).filter_map(|_| new_id().ok());
    std::iter::once(name.to_string()).chain(
        numbered
            .chain(random)
            .map(move |suffix| format!("{stem} ({suffix}){ext}")),
    )
}

/// Removes part files older than a day from `dir` -- leftovers of a crash
/// or power loss. Returns how many were removed. Blocking; meant for the
/// start of a receive session.
pub fn remove_stale_parts(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let is_part = name
            .to_str()
            .is_some_and(|n| n.starts_with(PART_PREFIX) && n.ends_with(PART_SUFFIX));
        let is_stale = || {
            entry
                .metadata()
                .ok()
                .filter(std::fs::Metadata::is_file)
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age > STALE_AFTER)
        };
        if is_part && is_stale() && std::fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }
    if removed > 0 {
        log::info!("{removed} alte unvollständige Empfangsdateien entfernt");
    }
    removed
}

/// Marks a received file as coming from another device, like a browser
/// download, so Windows SmartScreen / Office Protected View and macOS
/// Gatekeeper warn before it is opened or run. Best effort: some file
/// systems (FAT, exFAT) cannot store the mark, which must not fail the
/// upload.
fn mark_as_downloaded(path: &Path) {
    if let Err(e) = write_origin_mark(path) {
        log::debug!("Herkunftsmarkierung nicht möglich: {}", e.kind());
    }
}

/// Zone.Identifier stream, zone 3 = Internet.
#[cfg(windows)]
fn write_origin_mark(path: &Path) -> io::Result<()> {
    let mut stream = path.as_os_str().to_os_string();
    stream.push(":Zone.Identifier");
    std::fs::write(stream, "[ZoneTransfer]\r\nZoneId=3\r\n")
}

/// `com.apple.quarantine` attribute: flags 0081 (downloaded, not approved
/// by the user yet), time stamp, agent name.
#[cfg(target_os = "macos")]
fn write_origin_mark(path: &Path) -> io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let c_path = std::ffi::CString::new(path.as_os_str().as_bytes())?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    let value = format!("0081;{stamp:08x};QR Drop;");
    // SAFETY: both names are valid NUL-terminated C strings and `value`
    // outlives the call; pointer and length describe the same buffer.
    let rc = unsafe {
        libc::setxattr(
            c_path.as_ptr(),
            c"com.apple.quarantine".as_ptr(),
            value.as_ptr().cast(),
            value.len(),
            0,
            0,
        )
    };
    if rc == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
fn write_origin_mark(_path: &Path) -> io::Result<()> {
    Ok(())
}

/// Clears the hidden attribute of a part file before it gets its real name.
#[cfg(windows)]
fn reveal(path: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::{
        SetFileAttributesW, FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_NORMAL,
    };

    let attributes = std::fs::metadata(path)?.file_attributes() & !FILE_ATTRIBUTE_HIDDEN;
    // NORMAL is only valid alone, and required when nothing else is set.
    let attributes = if attributes == 0 {
        FILE_ATTRIBUTE_NORMAL
    } else {
        attributes
    };
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: `wide` is NUL-terminated and outlives the call.
    if unsafe { SetFileAttributesW(wide.as_ptr(), attributes) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Nothing to do: the final name has no leading dot.
#[cfg(not(windows))]
fn reveal(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part_in(dir: &Path, content: &[u8]) -> PathBuf {
        let part = new_part_path(dir).unwrap();
        std::fs::write(&part, content).unwrap();
        part
    }

    #[test]
    fn existing_files_are_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        let first = place_file(&part_in(d, b"1"), d, "a.jpg").unwrap();
        assert_eq!(first, d.join("a.jpg"));
        let second = place_file(&part_in(d, b"2"), d, "a.jpg").unwrap();
        assert_eq!(second, d.join("a (2).jpg"));
        let third = place_file(&part_in(d, b"3"), d, "a.jpg").unwrap();
        assert_eq!(third, d.join("a (3).jpg"));
        assert_eq!(std::fs::read(&first).unwrap(), b"1");
        assert_eq!(std::fs::read(&second).unwrap(), b"2");

        std::fs::create_dir(d.join("README")).unwrap();
        let readme = place_file(&part_in(d, b"x"), d, "README").unwrap();
        assert_eq!(readme, d.join("README (2)"));
    }

    #[test]
    fn candidates_end_with_random_names() {
        let names: Vec<String> = candidate_names("a.txt").collect();
        assert_eq!(names[0], "a.txt");
        assert_eq!(names[1], "a (2).txt");
        assert_eq!(names[998], "a (999).txt");
        assert_eq!(names.len(), 999 + RANDOM_ATTEMPTS as usize);
        assert!(names[999].starts_with("a (") && names[999].ends_with(").txt"));
    }

    #[cfg(unix)]
    #[test]
    fn dangling_symlinks_are_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        let outside = d.join("outside.txt");
        std::os::unix::fs::symlink(&outside, d.join("a.txt")).unwrap();
        let placed = place_file(&part_in(d, b"x"), d, "a.txt").unwrap();
        assert_eq!(placed, d.join("a (2).txt"));
        assert!(!outside.exists());
    }

    #[test]
    fn stale_parts_are_removed() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        let old = part_in(d, b"old");
        let fresh = part_in(d, b"fresh");
        let other = d.join("Urlaub.jpg");
        std::fs::write(&other, b"x").unwrap();
        let day_ago = std::time::SystemTime::now() - STALE_AFTER - Duration::from_secs(60);
        for path in [&old, &other] {
            std::fs::File::options()
                .write(true)
                .open(path)
                .unwrap()
                .set_modified(day_ago)
                .unwrap();
        }
        assert_eq!(remove_stale_parts(d), 1);
        assert!(!old.exists());
        assert!(fresh.exists());
        assert!(other.exists(), "only part files are touched");
        assert_eq!(remove_stale_parts(&d.join("missing")), 0);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn parts_are_hidden_until_placed_and_then_marked() {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_HIDDEN;

        let dir = tempfile::tempdir().unwrap();
        let part = new_part_path(dir.path()).unwrap();
        drop(open_part(&part, true).await.unwrap());
        let hidden =
            |p: &Path| std::fs::metadata(p).unwrap().file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0;
        assert!(hidden(&part));
        let placed = place_file(&part, dir.path(), "a.exe").unwrap();
        assert!(!hidden(&placed));
        let mut stream = placed.as_os_str().to_os_string();
        stream.push(":Zone.Identifier");
        let mark = std::fs::read_to_string(stream).unwrap();
        assert!(mark.contains("ZoneId=3"));
    }
}
