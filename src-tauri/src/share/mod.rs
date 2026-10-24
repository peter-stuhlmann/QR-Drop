//! Share sessions: what is shared, under which token, and for how long.
//!
//! Deliberately free of any Tauri types so the HTTP server can be tested on
//! its own. The UI side learns about changes through an [`EventSink`].
//!
//! - [`source`]: what a share contains ([`ShareSource`], [`FileKind`])
//! - [`text`]: classifying texts (link, secret, plain text)
//! - [`scan`]: turning paths into a single file or a ZIP listing
//! - [`manager`]: the registry of running shares, their lifetime and transfers
//! - [`progress`]: progress throttling, speed and delivered byte ranges

mod manager;
mod progress;
mod scan;
mod source;
mod text;

use std::path::PathBuf;
use std::sync::Arc;

use base64::Engine;
use serde::Serialize;

pub use manager::{
    ActiveShare, ShareConfig, ShareManager, DEFAULT_MAX_LIFETIME, DEFAULT_TTL, SECRET_TTL,
};
pub(crate) use manager::{Lookup, Transfer, TransferKind};
pub(crate) use progress::RateMeter;
pub(crate) use scan::FALLBACK_NAME;
pub(crate) use source::mime_for;
pub use source::{FileKind, ShareSource, ZipEntry};
pub use text::{looks_like_secret, text_preview, MAX_TEXT_BYTES};

/// 128 random bits, URL-safe Base64 without padding (22 characters).
pub(crate) fn new_token() -> Result<String, getrandom::Error> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes)?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
}

/// Short random id for windows and part files (48 bits, hex). Not a secret.
pub fn new_id() -> Result<String, getrandom::Error> {
    let mut bytes = [0u8; 6];
    getrandom::fill(&mut bytes)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Tokens are compared in constant time to not leak prefixes via timing.
pub(crate) fn token_eq(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

/// Events for the UI. Serialized to the frontend as-is.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ShareEvent {
    /// The phone opened the page.
    Connected {
        id: String,
    },
    Progress {
        id: String,
        sent: u64,
        total: u64,
        #[serde(rename = "bytesPerSec")]
        bytes_per_sec: u64,
    },
    /// A download or upload broke off before it finished (and no other
    /// transfer of the share is still running).
    Interrupted {
        id: String,
    },
    /// The single download (or text) was delivered completely.
    Done {
        id: String,
    },
    Expired {
        id: String,
    },
    /// The phone is uploading a file (part of a batch of `count` files).
    Upload {
        id: String,
        name: String,
        index: u32,
        count: u32,
        sent: u64,
        total: u64,
        #[serde(rename = "bytesPerSec")]
        bytes_per_sec: u64,
    },
    /// A file from the phone was saved.
    Received {
        id: String,
        name: String,
        size: u64,
        kind: FileKind,
        /// Where it was saved; kept on the Rust side ("Ordner öffnen").
        #[serde(skip)]
        path: PathBuf,
    },
    /// A text from the phone was put into the clipboard.
    TextReceived {
        id: String,
        chars: usize,
        /// First line; `None` for passwords, which stay masked.
        preview: Option<String>,
    },
}

impl ShareEvent {
    pub fn id(&self) -> &str {
        match self {
            ShareEvent::Connected { id }
            | ShareEvent::Progress { id, .. }
            | ShareEvent::Interrupted { id }
            | ShareEvent::Done { id }
            | ShareEvent::Expired { id }
            | ShareEvent::Upload { id, .. }
            | ShareEvent::Received { id, .. }
            | ShareEvent::TextReceived { id, .. } => id,
        }
    }
}

pub type EventSink = Arc<dyn Fn(ShareEvent) + Send + Sync>;

/// Puts a text from the phone into the computer's clipboard. Returns false
/// if that failed.
pub type TextSink = Arc<dyn Fn(&str) -> bool + Send + Sync>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_is_128_bit_urlsafe() {
        let t = new_token().unwrap();
        assert_eq!(t.len(), 22);
        assert!(t
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
        assert_ne!(t, new_token().unwrap());
    }

    #[test]
    fn token_comparison() {
        assert!(token_eq("abc", "abc"));
        assert!(!token_eq("abc", "abd"));
        assert!(!token_eq("abc", "ab"));
    }
}
