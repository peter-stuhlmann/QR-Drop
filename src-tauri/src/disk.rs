//! Free disk space, so an upload from the phone can be refused before it
//! fills the disk instead of failing at the very end.

use std::path::Path;

/// Bytes that always stay free (the system needs some room, too).
pub const RESERVE: u64 = 64 * 1024 * 1024;

/// Free bytes for the current user on the drive of `dir`, or `None` if
/// unknown. A missing folder is answered for its nearest existing parent.
pub fn available_space(dir: &Path) -> Option<u64> {
    let existing = dir.ancestors().find(|p| p.exists())?;
    query(existing)
}

#[cfg(windows)]
fn query(dir: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    let wide: Vec<u16> = dir
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut free: u64 = 0;
    // SAFETY: `wide` is NUL-terminated and outlives the call; the only out
    // pointer we pass is a valid, writable u64.
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut free,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    (ok != 0).then_some(free)
}

#[cfg(unix)]
fn query(dir: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;

    let path = std::ffi::CString::new(dir.as_os_str().as_bytes()).ok()?;
    // SAFETY: an all-zero statvfs is a valid value to be overwritten.
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `path` is a valid C string, `stat` a valid out pointer.
    if unsafe { libc::statvfs(path.as_ptr(), &mut stat) } != 0 {
        return None;
    }
    #[allow(clippy::unnecessary_cast)] // the field types differ per platform
    Some(stat.f_bavail as u64 * stat.f_frsize as u64)
}

#[cfg(not(any(windows, unix)))]
fn query(_dir: &Path) -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_space_for_existing_and_missing_folders() {
        let dir = tempfile::tempdir().unwrap();
        let free = available_space(dir.path()).unwrap();
        assert!(free > 0);
        let missing = dir.path().join("gibt es nicht").join("auch nicht");
        assert!(available_space(&missing).is_some());
    }
}
