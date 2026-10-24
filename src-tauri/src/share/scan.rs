//! Turning the selected paths into a share: one file as is, everything else
//! (folders, several paths) as a ZIP listing that is streamed later.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

use super::{ShareSource, ZipEntry};

/// Name for files whose own name is unusable (empty, only dots …).
pub(crate) const FALLBACK_NAME: &str = "Datei";

/// Files that never belong into a ZIP.
const JUNK_FILES: &[&str] = &[".DS_Store", "Thumbs.db", "desktop.ini", ".localized"];

impl ShareSource {
    /// Builds the source for one or more paths. A single file is served as
    /// is, everything else (folders, multiple paths) becomes a ZIP.
    /// Unreadable items inside folders are skipped, not fatal.
    pub fn from_paths(paths: &[PathBuf], zip_date: &str) -> io::Result<Self> {
        let paths: Vec<&PathBuf> = paths.iter().filter(|p| p.exists()).collect();
        if paths.is_empty() {
            return Err(io::Error::new(io::ErrorKind::NotFound, "no existing path"));
        }
        if paths.len() == 1 && paths[0].is_file() {
            let path = paths[0].clone();
            let size = std::fs::metadata(&path)?.len();
            let name = file_name(&path);
            return Ok(ShareSource::File { path, name, size });
        }

        let mut scan = Scan::default();
        for path in &paths {
            let base = unique_name(&mut scan.used_names, &file_name(path));
            if path.is_dir() {
                // The selected folder itself must be readable; only items
                // further down are skipped.
                let children = read_children(path)?;
                scan.push_dir(path, &base);
                scan.collect(children, &base);
            } else {
                let size = std::fs::metadata(path)?.len();
                scan.entries.push(ZipEntry {
                    path: (*path).clone(),
                    name: base,
                    size,
                    is_dir: false,
                });
            }
        }
        if scan.skipped > 0 {
            log::warn!(
                "{} nicht lesbare Elemente beim Packen übersprungen",
                scan.skipped
            );
        }
        let entries = scan.entries;
        let total = entries.iter().map(|e| e.size).sum();
        let file_count = entries.iter().filter(|e| !e.is_dir).count();
        let name = if paths.len() == 1 {
            format!("{}.zip", file_name(paths[0]))
        } else {
            format!("QR-Drop_{zip_date}.zip")
        };
        Ok(ShareSource::Zip {
            name,
            entries: entries.into(),
            total,
            file_count,
        })
    }
}

#[derive(Default)]
struct Scan {
    entries: Vec<ZipEntry>,
    /// Top-level names already used (several selected items may share one).
    used_names: HashMap<String, usize>,
    /// Unreadable files and folders, only counted for the log.
    skipped: usize,
}

impl Scan {
    fn push_dir(&mut self, path: &Path, zip_name: &str) {
        self.entries.push(ZipEntry {
            path: path.to_path_buf(),
            name: format!("{zip_name}/"),
            size: 0,
            is_dir: true,
        });
    }

    fn collect(&mut self, children: Vec<std::fs::DirEntry>, prefix: &str) {
        for child in children {
            let name = child.file_name().to_string_lossy().into_owned();
            if JUNK_FILES.contains(&name.as_str()) {
                continue;
            }
            // Never follow symlinks: avoids loops and leaking files outside the folder.
            let Ok(file_type) = child.file_type() else {
                self.skipped += 1;
                continue;
            };
            if file_type.is_symlink() {
                continue;
            }
            let path = child.path();
            let zip_name = format!("{prefix}/{name}");
            if file_type.is_dir() {
                match read_children(&path) {
                    Ok(grandchildren) => {
                        self.push_dir(&path, &zip_name);
                        self.collect(grandchildren, &zip_name);
                    }
                    Err(_) => self.skipped += 1,
                }
            } else if file_type.is_file() {
                match child.metadata() {
                    Ok(meta) => self.entries.push(ZipEntry {
                        path,
                        name: zip_name,
                        size: meta.len(),
                        is_dir: false,
                    }),
                    Err(_) => self.skipped += 1,
                }
            }
        }
    }
}

/// Folder contents, sorted by name so the archive is reproducible.
fn read_children(dir: &Path) -> io::Result<Vec<std::fs::DirEntry>> {
    let mut children: Vec<_> = std::fs::read_dir(dir)?.filter_map(Result::ok).collect();
    children.sort_by_key(|e| e.file_name());
    Ok(children)
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| FALLBACK_NAME.to_string())
}

fn unique_name(used: &mut HashMap<String, usize>, name: &str) -> String {
    let count = used.entry(name.to_string()).or_insert(0);
    *count += 1;
    if *count == 1 {
        name.to_string()
    } else {
        match name.rsplit_once('.') {
            Some((stem, ext)) if !stem.is_empty() => format!("{stem} ({count}).{ext}"),
            _ => format!("{name} ({count})"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_names_get_suffix() {
        let mut used = HashMap::new();
        assert_eq!(unique_name(&mut used, "a.jpg"), "a.jpg");
        assert_eq!(unique_name(&mut used, "a.jpg"), "a (2).jpg");
        assert_eq!(unique_name(&mut used, "Ordner"), "Ordner");
        assert_eq!(unique_name(&mut used, "Ordner"), "Ordner (2)");
    }

    #[test]
    fn folder_becomes_zip_without_junk_and_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Urlaub");
        std::fs::create_dir_all(root.join("Tag 1")).unwrap();
        std::fs::write(root.join("Tag 1/bild.jpg"), b"jpg").unwrap();
        std::fs::write(root.join("notiz.txt"), b"hallo").unwrap();
        std::fs::write(root.join(".DS_Store"), b"x").unwrap();
        let src = ShareSource::from_paths(&[root], "2026-10-21").unwrap();
        let ShareSource::Zip {
            name,
            entries,
            total,
            file_count,
        } = src
        else {
            panic!("expected zip")
        };
        assert_eq!(name, "Urlaub.zip");
        let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Urlaub/",
                "Urlaub/Tag 1/",
                "Urlaub/Tag 1/bild.jpg",
                "Urlaub/notiz.txt"
            ]
        );
        assert_eq!(total, 8);
        assert_eq!(file_count, 2);
    }

    #[test]
    fn multiple_files_get_dated_zip_name() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        std::fs::write(&a, b"a").unwrap();
        std::fs::write(&b, b"b").unwrap();
        let src = ShareSource::from_paths(&[a, b], "2026-10-21").unwrap();
        assert_eq!(src.name(), "QR-Drop_2026-10-21.zip");
        assert!(src.is_zip());
    }

    #[test]
    fn missing_paths_are_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("weg");
        assert!(ShareSource::from_paths(&[missing], "x").is_err());
    }
}
