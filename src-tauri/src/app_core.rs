//! Application state shared by commands, windows and the server, and the
//! view model of a session window.

use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::args::Collector;
use crate::batch::Batches;
use crate::net::{self, Candidate, IfaceKind};
use crate::qr;
use crate::server::receive::Uploads;
use crate::server::ServerHandle;
use crate::share::{text_preview, ActiveShare, FileKind, ShareManager, ShareSource};

/// Characters of a text shown as preview in the window.
const TEXT_PREVIEW_CHARS: usize = 80;

/// The shared state, if setup has run. Every entry point (commands, events,
/// callbacks) goes through this instead of `app.state()`, which would panic
/// before setup.
pub fn get(app: &AppHandle) -> Option<Arc<AppCore>> {
    app.try_state::<Arc<AppCore>>()
        .map(|core| core.inner().clone())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkOption {
    pub ip: String,
    pub name: String,
    pub kind: IfaceKind,
}

impl From<&Candidate> for NetworkOption {
    fn from(c: &Candidate) -> Self {
        Self {
            ip: c.ip.to_string(),
            name: c.name.clone(),
            kind: c.kind,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ShareStatus {
    Preparing,
    Ready,
    Expired,
    Error,
}

/// What a session window shows. Serialized as `file`, `text`, `url` or
/// `receive`.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ShareContent {
    File,
    Text,
    Url,
    /// Phone → computer.
    Receive,
}

/// Machine-readable reason of the `Error` status; the UI maps it to a
/// human text (`error.<code>.*` in `src/i18n`).
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ShareError {
    /// No usable LAN address.
    NoNetwork,
    /// The files vanished or are unreadable.
    NotFound,
    /// The QR code could not be generated.
    Qr,
    /// Anything else (e.g. no randomness for a token).
    Generic,
    ClipboardEmpty,
    TextTooLarge,
}

/// Everything a session window needs to render.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareInfo {
    pub id: String,
    pub status: ShareStatus,
    pub error: Option<ShareError>,
    pub name: String,
    pub size: u64,
    pub kind: FileKind,
    pub is_zip: bool,
    pub file_count: usize,
    pub content: ShareContent,
    /// Masked in the UI and on the phone.
    pub secret: bool,
    /// Number of characters for texts.
    pub char_count: usize,
    /// Shortened first line of a text (never for secrets) or the link.
    pub preview: Option<String>,
    pub url: Option<String>,
    pub qr_svg: Option<String>,
    pub expires_in_ms: u64,
    pub ttl_ms: u64,
    pub networks: Vec<NetworkOption>,
    pub selected_ip: Option<String>,
}

impl ShareInfo {
    fn new(id: &str, status: ShareStatus, ttl: Duration) -> Self {
        Self {
            id: id.to_string(),
            status,
            error: None,
            name: String::new(),
            size: 0,
            kind: FileKind::File,
            is_zip: false,
            file_count: 0,
            content: ShareContent::File,
            secret: false,
            char_count: 0,
            preview: None,
            url: None,
            qr_svg: None,
            expires_in_ms: 0,
            ttl_ms: millis(ttl),
            networks: Vec::new(),
            selected_ip: None,
        }
    }

    fn fail(&mut self, error: ShareError) {
        self.status = ShareStatus::Error;
        self.error = Some(error);
    }

    /// Name, size and content description of a running share.
    fn describe(&mut self, share: &ActiveShare) {
        let source = &*share.source;
        self.name = source.name().to_string();
        self.size = source.size();
        self.kind = source.kind();
        self.is_zip = source.is_zip();
        self.ttl_ms = millis(share.ttl);
        match source {
            ShareSource::File { .. } => self.file_count = 1,
            ShareSource::Zip { file_count, .. } => self.file_count = *file_count,
            ShareSource::Text { text, secret } => {
                self.content = ShareContent::Text;
                self.secret = *secret;
                self.char_count = text.chars().count();
                self.preview = (!secret).then(|| text_preview(text, TEXT_PREVIEW_CHARS));
            }
            ShareSource::Url { url } => {
                // The QR code is the link itself: works without this server
                // and without a shared Wi-Fi, and never expires.
                self.content = ShareContent::Url;
                self.char_count = url.chars().count();
                self.preview = Some(url.clone());
                self.ttl_ms = 0;
            }
            ShareSource::Receive { .. } => self.content = ShareContent::Receive,
        }
    }
}

fn millis(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

/// Latest scan generation per share: a slow scan never overwrites a newer
/// one (a multi-selection grows a share into a ZIP). Generations come from
/// one global counter, so they never repeat -- not even for a share that
/// was forgotten and whose old scan finishes late.
#[derive(Debug, Default)]
struct ScanGenerations {
    counter: AtomicU64,
    latest: Mutex<HashMap<String, u64>>,
}

impl ScanGenerations {
    fn next(&self, id: &str) -> u64 {
        let generation = self.counter.fetch_add(1, Ordering::Relaxed) + 1;
        lock(&self.latest).insert(id.to_string(), generation);
        generation
    }

    fn is_current(&self, id: &str, generation: u64) -> bool {
        lock(&self.latest).get(id) == Some(&generation)
    }

    fn forget(&self, id: &str) {
        lock(&self.latest).remove(id);
    }
}

/// A share whose files are still being scanned.
#[derive(Debug, Clone)]
struct Pending {
    name: String,
    count: usize,
}

pub struct AppCore {
    pub manager: Arc<ShareManager>,
    /// Unfinished uploads from the phone (shared with the server).
    pub uploads: Arc<Uploads>,
    pub collector: Arc<Collector>,
    /// Multi-selections whose first paths are shown right away.
    pub batches: Batches,
    port: u16,
    server: Mutex<Option<ServerHandle>>,
    pending: Mutex<HashMap<String, Pending>>,
    failed: Mutex<HashMap<String, ShareError>>,
    selected_ip: Mutex<HashMap<String, Ipv4Addr>>,
    /// Files saved by receive sessions, for "Ordner öffnen".
    received: Mutex<HashMap<String, Vec<PathBuf>>>,
    /// When the last request (process start or second instance) arrived;
    /// used to log how fast its window became ready (CLAUDE.md §12).
    last_request: Mutex<Option<Instant>>,
    scans: ScanGenerations,
    /// Last QR code per share with the URL it encodes; the window asks for
    /// its state often, the URL only changes with a new token or address.
    qr_cache: Mutex<HashMap<String, (String, String)>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl AppCore {
    pub fn new(manager: Arc<ShareManager>, server: ServerHandle, uploads: Arc<Uploads>) -> Self {
        Self {
            manager,
            uploads,
            collector: Collector::new(),
            batches: Batches::default(),
            port: server.port,
            server: Mutex::new(Some(server)),
            pending: Mutex::default(),
            failed: Mutex::default(),
            selected_ip: Mutex::default(),
            received: Mutex::default(),
            last_request: Mutex::default(),
            scans: ScanGenerations::default(),
            qr_cache: Mutex::default(),
        }
    }

    pub fn shutdown(&self) {
        if let Some(mut server) = lock(&self.server).take() {
            server.shutdown();
        }
    }

    pub fn set_pending(&self, id: &str, name: String, count: usize) {
        lock(&self.pending).insert(id.to_string(), Pending { name, count });
    }

    pub fn clear_pending(&self, id: &str) {
        lock(&self.pending).remove(id);
    }

    pub fn set_failed(&self, id: &str, reason: ShareError) {
        lock(&self.failed).insert(id.to_string(), reason);
    }

    pub fn clear_failed(&self, id: &str) {
        lock(&self.failed).remove(id);
    }

    /// Drops everything known about a session (its window is gone).
    pub fn forget(&self, id: &str) {
        self.manager.remove(id);
        // Closing a receive window discards half-finished uploads.
        self.uploads.discard_session(id);
        lock(&self.pending).remove(id);
        lock(&self.failed).remove(id);
        lock(&self.selected_ip).remove(id);
        lock(&self.received).remove(id);
        self.scans.forget(id);
        lock(&self.qr_cache).remove(id);
    }

    /// Starts a new scan generation for a share and returns it.
    pub fn next_scan(&self, id: &str) -> u64 {
        self.scans.next(id)
    }

    pub fn is_current_scan(&self, id: &str, generation: u64) -> bool {
        self.scans.is_current(id, generation)
    }

    pub fn note_request(&self, at: Instant) {
        *lock(&self.last_request) = Some(at);
    }

    /// Time since the pending request, once (the first window that loads).
    pub fn take_request_latency(&self) -> Option<Duration> {
        lock(&self.last_request).take().map(|t| t.elapsed())
    }

    pub fn record_received(&self, id: &str, path: PathBuf) {
        lock(&self.received)
            .entry(id.to_string())
            .or_default()
            .push(path);
    }

    pub fn received_paths(&self, id: &str) -> Vec<PathBuf> {
        lock(&self.received).get(id).cloned().unwrap_or_default()
    }

    pub fn select_ip(&self, id: &str, ip: Ipv4Addr) {
        lock(&self.selected_ip).insert(id.to_string(), ip);
    }

    /// Whether any session is being prepared or running.
    pub fn has_sessions(&self) -> bool {
        !lock(&self.pending).is_empty() || !self.manager.is_empty()
    }

    /// Builds the view model of a session window.
    pub fn info(&self, id: &str) -> Option<ShareInfo> {
        let default_ttl = self.manager.config().ttl;
        if let Some(reason) = lock(&self.failed).get(id).copied() {
            let mut info = ShareInfo::new(id, ShareStatus::Error, default_ttl);
            info.error = Some(reason);
            return Some(info);
        }
        if let Some(pending) = lock(&self.pending).get(id).cloned() {
            let mut info = ShareInfo::new(id, ShareStatus::Preparing, default_ttl);
            info.name = pending.name;
            info.file_count = pending.count;
            info.is_zip = pending.count > 1;
            return Some(info);
        }

        let share = self.manager.get(id)?;
        let mut info = ShareInfo::new(id, ShareStatus::Ready, share.ttl);
        info.describe(&share);
        if let ShareSource::Url { url } = &*share.source {
            self.attach_qr(&mut info, url.clone());
            return Some(info);
        }
        if self.manager.is_expired(id) {
            info.status = ShareStatus::Expired;
            return Some(info);
        }
        info.expires_in_ms = millis(share.expires_at.saturating_duration_since(Instant::now()));
        self.attach_address(&mut info, &share);
        Some(info)
    }

    /// The LAN addresses to choose from and the share URL on the chosen one.
    fn attach_address(&self, info: &mut ShareInfo, share: &ActiveShare) {
        let candidates = net::lan_candidates();
        info.networks = candidates.iter().map(NetworkOption::from).collect();
        let chosen = lock(&self.selected_ip).get(&share.id).copied();
        let ip = chosen
            .filter(|ip| candidates.iter().any(|c| c.ip == *ip))
            .or_else(|| candidates.first().map(|c| c.ip));
        let Some(ip) = ip else {
            info.fail(ShareError::NoNetwork);
            return;
        };
        let url = format!("http://{ip}:{}/s/{}", self.port, share.token);
        if cfg!(debug_assertions) {
            // The token is a secret: only dev builds log it.
            log::debug!("Share-URL: {url}");
        }
        info.selected_ip = Some(ip.to_string());
        self.attach_qr(info, url);
    }

    /// Sets the URL and its QR code (cached per share).
    fn attach_qr(&self, info: &mut ShareInfo, url: String) {
        match self.qr_svg(&info.id, &url) {
            Some(svg) => info.qr_svg = Some(svg),
            None => info.fail(ShareError::Qr),
        }
        info.url = Some(url);
    }

    fn qr_svg(&self, id: &str, url: &str) -> Option<String> {
        if let Some((cached_url, svg)) = lock(&self.qr_cache).get(id) {
            if cached_url == url {
                return Some(svg.clone());
            }
        }
        let svg = qr::svg(url).ok()?;
        lock(&self.qr_cache).insert(id.to_string(), (url.to_string(), svg.clone()));
        Some(svg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_generations_never_repeat() {
        let scans = ScanGenerations::default();
        let first = scans.next("a");
        assert!(scans.is_current("a", first));
        let second = scans.next("a");
        assert!(!scans.is_current("a", first));
        assert!(scans.is_current("a", second));
        // Forgotten (window closed): a late scan is stale, and so is any
        // generation handed out before a restart of the same id.
        scans.forget("a");
        assert!(!scans.is_current("a", second));
        let third = scans.next("a");
        assert!(third > second);
        assert!(!scans.is_current("a", first));
    }

    #[test]
    fn errors_serialize_as_codes() {
        let json = serde_json::to_string(&[
            ShareError::NoNetwork,
            ShareError::NotFound,
            ShareError::Qr,
            ShareError::Generic,
            ShareError::ClipboardEmpty,
            ShareError::TextTooLarge,
        ])
        .unwrap();
        assert_eq!(
            json,
            r#"["no_network","not_found","qr","generic","clipboard_empty","text_too_large"]"#
        );
        let content = serde_json::to_string(&[
            ShareContent::File,
            ShareContent::Text,
            ShareContent::Url,
            ShareContent::Receive,
        ])
        .unwrap();
        assert_eq!(content, r#"["file","text","url","receive"]"#);
    }
}
