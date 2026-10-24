//! What a share contains.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;

#[derive(Debug, Clone)]
pub struct ZipEntry {
    /// Absolute path on disk.
    pub path: PathBuf,
    /// Path inside the archive, `/`-separated, directories end with `/`.
    pub name: String,
    pub size: u64,
    pub is_dir: bool,
}

#[derive(Clone)]
pub enum ShareSource {
    File {
        path: PathBuf,
        name: String,
        size: u64,
    },
    Zip {
        name: String,
        /// Shared, so every download streams the same listing without a copy.
        entries: Arc<[ZipEntry]>,
        /// Sum of all file sizes (uncompressed).
        total: u64,
        file_count: usize,
    },
    /// Text from the clipboard or a selection, fetched by the landing page.
    Text { text: String, secret: bool },
    /// A link: the QR code contains the URL itself, no server involved.
    Url { url: String },
    /// The other direction: the phone uploads files (into `dir`) and texts
    /// (into the clipboard). Stays open for several uploads.
    Receive { dir: PathBuf },
}

/// Never prints the shared text: it may be a password and Debug output can
/// end up in logs.
impl fmt::Debug for ShareSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ShareSource::File { name, size, .. } => f
                .debug_struct("File")
                .field("name", name)
                .field("size", size)
                .finish_non_exhaustive(),
            ShareSource::Zip {
                name, file_count, ..
            } => f
                .debug_struct("Zip")
                .field("name", name)
                .field("file_count", file_count)
                .finish_non_exhaustive(),
            ShareSource::Text { text, secret } => f
                .debug_struct("Text")
                .field("chars", &text.chars().count())
                .field("secret", secret)
                .finish_non_exhaustive(),
            ShareSource::Url { .. } => f.debug_struct("Url").finish_non_exhaustive(),
            ShareSource::Receive { .. } => f.debug_struct("Receive").finish_non_exhaustive(),
        }
    }
}

/// MIME type by file extension, `application/octet-stream` if unknown.
pub(crate) fn mime_for(path: &Path) -> &'static str {
    mime_guess::from_path(path)
        .first_raw()
        .unwrap_or("application/octet-stream")
}

impl ShareSource {
    pub fn is_secret(&self) -> bool {
        matches!(self, ShareSource::Text { secret: true, .. })
    }

    /// File name; for texts a neutral label (the UI localizes it).
    pub fn name(&self) -> &str {
        match self {
            ShareSource::File { name, .. } | ShareSource::Zip { name, .. } => name,
            ShareSource::Text { .. } => "Text",
            ShareSource::Url { url } => url,
            ShareSource::Receive { dir } => {
                dir.file_name().and_then(|n| n.to_str()).unwrap_or_default()
            }
        }
    }

    /// Exact size for single files, uncompressed total for ZIPs, bytes for texts.
    pub fn size(&self) -> u64 {
        match self {
            ShareSource::File { size, .. } => *size,
            ShareSource::Zip { total, .. } => *total,
            ShareSource::Text { text, .. } => text.len() as u64,
            ShareSource::Url { url } => url.len() as u64,
            ShareSource::Receive { .. } => 0,
        }
    }

    pub fn is_zip(&self) -> bool {
        matches!(self, ShareSource::Zip { .. })
    }

    /// Coarse type used for icons in the UI and on the landing page.
    pub fn kind(&self) -> FileKind {
        match self {
            ShareSource::Zip { .. } => FileKind::Archive,
            ShareSource::File { path, .. } => FileKind::from_mime(mime_for(path)),
            ShareSource::Text { secret: true, .. } => FileKind::Secret,
            ShareSource::Text { .. } => FileKind::Text,
            ShareSource::Url { .. } => FileKind::Link,
            ShareSource::Receive { .. } => FileKind::Receive,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FileKind {
    Image,
    Video,
    Audio,
    Document,
    Archive,
    File,
    Text,
    Secret,
    Link,
    /// "Vom Handy empfangen" session.
    Receive,
}

impl FileKind {
    pub fn from_mime(mime: &str) -> Self {
        let top = mime.split('/').next().unwrap_or_default();
        match top {
            "image" => FileKind::Image,
            "video" => FileKind::Video,
            "audio" => FileKind::Audio,
            _ if mime.contains("zip") || mime.contains("compressed") || mime.contains("x-7z") => {
                FileKind::Archive
            }
            _ if mime.starts_with("text/")
                || mime.contains("pdf")
                || mime.contains("document")
                || mime.contains("msword")
                || mime.contains("sheet")
                || mime.contains("presentation") =>
            {
                FileKind::Document
            }
            _ => FileKind::File,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_kinds() {
        assert_eq!(FileKind::from_mime("image/jpeg"), FileKind::Image);
        assert_eq!(FileKind::from_mime("video/mp4"), FileKind::Video);
        assert_eq!(FileKind::from_mime("application/pdf"), FileKind::Document);
        assert_eq!(FileKind::from_mime("application/zip"), FileKind::Archive);
        assert_eq!(
            FileKind::from_mime("application/octet-stream"),
            FileKind::File
        );
        assert_eq!(mime_for(Path::new("README")), "application/octet-stream");
    }

    #[test]
    fn debug_never_shows_texts() {
        let s = ShareSource::Text {
            text: "Tr0ub4dor&3".into(),
            secret: true,
        };
        let out = format!("{s:?}");
        assert!(!out.contains("Tr0ub4dor"), "{out}");
    }
}
