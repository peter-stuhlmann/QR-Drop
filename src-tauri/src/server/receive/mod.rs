//! The other direction: the phone sends files (saved into the target
//! folder, usually "Downloads") and texts (put into the clipboard).
//!
//! Everything from the phone is untrusted: file names are reduced to a
//! single safe name ([`names`]), nothing is ever overwritten and a file only
//! appears under its real name once it arrived completely ([`store`]).
//! Uploads with a key can be resumed and taken over ([`uploads`]).

mod names;
mod store;
mod uploads;

use std::io;
use std::path::{Path as FsPath, PathBuf};

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use futures_util::StreamExt;
use serde::Deserialize;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};

pub use store::remove_stale_parts;
pub use uploads::Uploads;

use super::routes::connect;
use super::{json, json_error, AppState};
use crate::share::{
    looks_like_secret, mime_for, text_preview, ActiveShare, FileKind, RateMeter, ShareEvent,
    ShareSource, Transfer, TransferKind, MAX_TEXT_BYTES,
};
use names::sanitize_file_name;
use uploads::{Claim, Finished};

/// Bytes collected in memory before they are written to the part file.
const WRITE_BUFFER: usize = 256 * 1024;

/// How far past the limit an oversized text is still read, see
/// [`read_limited`].
const TEXT_DRAIN_FACTOR: usize = 4;

fn gone() -> Response {
    json_error(StatusCode::NOT_FOUND, "gone")
}

/// The receive session for a token, if it is one.
fn session(state: &AppState, token: &str) -> Option<(ActiveShare, PathBuf)> {
    let share = state.manager.by_token(token)?;
    let ShareSource::Receive { dir } = &*share.source else {
        return None;
    };
    let dir = dir.clone();
    Some((share, dir))
}

async fn file_len(path: &FsPath) -> u64 {
    tokio::fs::metadata(path)
        .await
        .map(|m| m.len())
        .unwrap_or(0)
}

/// The phone's key for a file: 8–64 URL-safe characters, anything else is
/// ignored (the upload then simply cannot be resumed).
fn clean_key(raw: &str) -> Option<String> {
    let ok = (8..=64).contains(&raw.len())
        && raw
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    ok.then(|| raw.to_string())
}

#[derive(Debug, Deserialize)]
pub(super) struct CheckParams {
    #[serde(default)]
    size: u64,
}

/// `GET /s/{token}/check?size=…`: is there room for the next file? The page
/// asks before each upload -- refusing the upload itself would only look
/// like a broken connection on the phone. Does not keep the session alive.
pub(super) async fn check(
    State(state): State<AppState>,
    Path(token): Path<String>,
    Query(params): Query<CheckParams>,
) -> Response {
    let Some((_, dir)) = session(&state, &token) else {
        return gone();
    };
    let free = tokio::task::spawn_blocking(move || crate::disk::available_space(&dir))
        .await
        .ok()
        .flatten();
    match free {
        Some(free) if params.size.saturating_add(crate::disk::RESERVE) > free => {
            json_error(StatusCode::INSUFFICIENT_STORAGE, "disk_full")
        }
        _ => StatusCode::NO_CONTENT.into_response(),
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct OffsetParams {
    key: Option<String>,
}

/// `GET /s/{token}/upload?key=…`: how many bytes of this file the computer
/// already has, so the phone continues there instead of starting over. A
/// file that already arrived completely is reported as done.
pub(super) async fn upload_offset(
    State(state): State<AppState>,
    Path(token): Path<String>,
    Query(params): Query<OffsetParams>,
) -> Response {
    let Some((share, _)) = session(&state, &token) else {
        return gone();
    };
    let Some(key) = params.key.as_deref().and_then(clean_key) else {
        return json(StatusCode::OK, serde_json::json!({ "offset": 0 }));
    };
    if let Some(done) = state.uploads.finished(&share.id, &key) {
        return json(
            StatusCode::OK,
            serde_json::json!({ "offset": done.size, "done": true, "name": done.name }),
        );
    }
    let offset = match state.uploads.part_path(&share.id, &key) {
        Some(path) => file_len(&path).await,
        None => 0,
    };
    json(StatusCode::OK, serde_json::json!({ "offset": offset }))
}

fn one() -> u32 {
    1
}

/// Sent by the upload page along with each file of a batch.
#[derive(Debug, Deserialize)]
pub(super) struct UploadParams {
    name: Option<String>,
    /// Position in the batch, 1-based.
    #[serde(default = "one")]
    i: u32,
    /// Files in the batch.
    #[serde(default = "one")]
    n: u32,
    /// Bytes of the whole batch.
    #[serde(default)]
    total: u64,
    /// Bytes of the batch before this file.
    #[serde(default)]
    offset: u64,
    /// The phone's key for this file; makes the upload resumable.
    key: Option<String>,
    /// Size of the whole file.
    size: Option<u64>,
    /// Where this request continues the file (bytes the computer has).
    #[serde(default)]
    from: u64,
}

enum Outcome {
    Saved {
        path: PathBuf,
        size: u64,
    },
    /// An earlier request already saved this file under `name`.
    AlreadySaved {
        name: String,
    },
    /// The request ended cleanly before the file was complete.
    Partial {
        offset: u64,
    },
}

enum Failure {
    /// The phone resumed at the wrong position; this is the right one.
    Offset(u64),
    TooManyUnfinished,
    /// Ended short of the announced size and cannot be resumed.
    Incomplete,
    /// More data than the announced size.
    Oversized,
    /// A newer request for the same file took over.
    Superseded,
    /// The window was closed on the computer.
    Cancelled,
    Io(io::Error),
}

impl From<io::Error> for Failure {
    fn from(e: io::Error) -> Self {
        Failure::Io(e)
    }
}

/// Collects body chunks and writes them to the part file in larger blocks.
/// For a keyed upload every write first checks that the request still owns
/// the part; on takeover or error the buffered rest is simply dropped (the
/// phone asks for the offset and sends it again).
struct PartWriter<'a> {
    file: tokio::fs::File,
    buf: Vec<u8>,
    claim: Option<&'a Claim>,
}

impl<'a> PartWriter<'a> {
    fn new(file: tokio::fs::File, claim: Option<&'a Claim>) -> Self {
        Self {
            file,
            buf: Vec::with_capacity(WRITE_BUFFER),
            claim,
        }
    }

    async fn write(&mut self, chunk: &[u8]) -> Result<(), Failure> {
        self.buf.extend_from_slice(chunk);
        if self.buf.len() >= WRITE_BUFFER {
            let _lock = match self.claim {
                Some(claim) => Some(claim.part.file_lock.lock().await),
                None => None,
            };
            self.flush_owned().await?;
        }
        Ok(())
    }

    /// Writes the buffer out. The caller holds the part's file lock.
    async fn flush_owned(&mut self) -> Result<(), Failure> {
        if self.claim.is_some_and(|claim| !claim.is_current()) {
            return Err(Failure::Superseded);
        }
        self.file.write_all(&self.buf).await?;
        // Wait until the data is in the file, not just handed to Tokio,
        // before the lock is released.
        self.file.flush().await?;
        self.buf.clear();
        Ok(())
    }

    /// Writes the rest, syncs and returns the length on disk. The caller
    /// holds the part's file lock.
    async fn complete(mut self) -> Result<u64, Failure> {
        self.flush_owned().await?;
        self.file.sync_all().await?;
        Ok(self.file.metadata().await?.len())
    }
}

/// Everything one upload request needs.
struct UploadCtx<'a> {
    state: &'a AppState,
    share: &'a ActiveShare,
    dir: &'a FsPath,
    /// Sanitized file name; the saved one may get a " (2)" suffix.
    name: String,
    params: &'a UploadParams,
    /// Content-Length, the progress total when `size` is missing.
    content_length: Option<u64>,
}

/// `POST /s/{token}/upload?name=…&i=…&n=…&total=…&offset=…&key=…&size=…&from=…`
/// with the raw file (or its rest, from `from` on) as body -- no multipart,
/// the page streams the file as it is.
pub(super) async fn upload(
    State(state): State<AppState>,
    Path(token): Path<String>,
    Query(params): Query<UploadParams>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let Some((share, dir)) = session(&state, &token) else {
        return gone();
    };
    connect(&state, &share);
    let ctx = UploadCtx {
        state: &state,
        share: &share,
        dir: &dir,
        name: sanitize_file_name(params.name.as_deref().unwrap_or_default()),
        params: &params,
        content_length: headers
            .get(header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok()),
    };
    let mut transfer = state
        .manager
        .begin_transfer(&share.id, TransferKind::Upload);
    let result = match params.key.as_deref().and_then(clean_key) {
        Some(key) => ctx.receive_resumable(&transfer, &key, body).await,
        None => ctx.receive_once(&transfer, body).await,
    };
    ctx.respond(result, &mut transfer)
}

impl UploadCtx<'_> {
    /// An upload without key: all or nothing.
    async fn receive_once(&self, transfer: &Transfer, body: Body) -> Result<Outcome, Failure> {
        if self.params.from != 0 {
            return Err(Failure::Offset(0));
        }
        tokio::fs::create_dir_all(self.dir).await?;
        let part = store::new_part_path(self.dir)?;
        let file = store::open_part(&part, true).await?;
        let result = async {
            let mut writer = PartWriter::new(file, None);
            self.copy_body(body, &mut writer, transfer, 0).await?;
            let have = writer.complete().await?;
            self.check_size(have)?;
            Ok(self.save(&part, have).await?)
        }
        .await;
        if result.is_err() {
            let _ = tokio::fs::remove_file(&part).await;
        }
        result
    }

    /// An upload with key: continues at `from`, keeps what was written when
    /// the connection breaks, and takes over from an older request for the
    /// same file.
    async fn receive_resumable(
        &self,
        transfer: &Transfer,
        key: &str,
        body: Body,
    ) -> Result<Outcome, Failure> {
        let (uploads, session, from) = (&self.state.uploads, &self.share.id, self.params.from);
        if let Some(done) = uploads.finished(session, key) {
            return Ok(Outcome::AlreadySaved { name: done.name });
        }
        tokio::fs::create_dir_all(self.dir).await?;
        let claim = uploads
            .claim(session, key, store::new_part_path(self.dir)?)
            .ok_or(Failure::TooManyUnfinished)?;
        let file = {
            let _lock = claim.part.file_lock.lock().await;
            // The request this one took over may have just completed it.
            if let Some(done) = uploads.finished(session, key) {
                return Ok(Outcome::AlreadySaved { name: done.name });
            }
            if !claim.is_current() {
                return Err(Failure::Superseded);
            }
            let have = file_len(&claim.part.path).await;
            if have != from {
                return Err(Failure::Offset(have));
            }
            let mut file = store::open_part(&claim.part.path, false).await?;
            // Exactly at `from`, whatever an older writer left behind.
            file.set_len(from).await?;
            file.seek(io::SeekFrom::Start(from)).await?;
            file
        };

        let mut writer = PartWriter::new(file, Some(&claim));
        if let Err(failure) = self.copy_body(body, &mut writer, transfer, from).await {
            if matches!(failure, Failure::Cancelled) {
                uploads.forget(session, key);
                let _ = tokio::fs::remove_file(&claim.part.path).await;
            }
            // Otherwise the part stays for resuming.
            return Err(failure);
        }

        let _lock = claim.part.file_lock.lock().await;
        let have = writer.complete().await?;
        if self.params.size.is_some_and(|size| have < size) {
            return Ok(Outcome::Partial { offset: have });
        }
        let saved = match self.check_size(have) {
            Ok(()) => self
                .save(&claim.part.path, have)
                .await
                .map_err(Failure::from),
            Err(failure) => Err(failure),
        };
        match &saved {
            Ok(Outcome::Saved { path, size }) => uploads.finish(
                session,
                key,
                Finished {
                    name: file_name(path),
                    size: *size,
                },
            ),
            _ => {
                uploads.forget(session, key);
                let _ = tokio::fs::remove_file(&claim.part.path).await;
            }
        }
        saved
    }

    /// The file must have exactly the announced size, if one was announced.
    fn check_size(&self, have: u64) -> Result<(), Failure> {
        match self.params.size {
            Some(size) if have < size => Err(Failure::Incomplete),
            Some(size) if have > size => Err(Failure::Oversized),
            _ => Ok(()),
        }
    }

    /// Copies the request body into `writer` and reports progress. `from`
    /// is where this request continues the file.
    async fn copy_body(
        &self,
        body: Body,
        writer: &mut PartWriter<'_>,
        transfer: &Transfer,
        from: u64,
    ) -> Result<(), Failure> {
        let mut stream = body.into_data_stream();
        let mut meter = RateMeter::new(from);
        let mut have = from;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| io::Error::new(io::ErrorKind::ConnectionAborted, e))?;
            // The window was closed ("Abbrechen"): stop instead of saving.
            if transfer.is_cancelled() {
                return Err(Failure::Cancelled);
            }
            writer.write(&chunk).await?;
            have = have.saturating_add(chunk.len() as u64);
            if let Some(rate) = meter.sample(have, false) {
                // Real data keeps the session alive (idle timeout).
                self.state.manager.touch(&self.share.id);
                self.report(have, rate);
            }
        }
        if let Some(rate) = meter.sample(have, true) {
            self.report(have, rate);
        }
        Ok(())
    }

    /// Progress of this file within its batch.
    fn report(&self, have: u64, bytes_per_sec: u64) {
        let p = self.params;
        let sent = p.offset.saturating_add(have);
        let file_size = p
            .size
            .unwrap_or_else(|| p.from.saturating_add(self.content_length.unwrap_or(0)));
        (self.state.sink)(ShareEvent::Upload {
            id: self.share.id.clone(),
            name: self.name.clone(),
            index: p.i.max(1),
            count: p.n.max(p.i).max(1),
            sent,
            total: p.total.max(p.offset.saturating_add(file_size)).max(sent),
            bytes_per_sec,
        });
    }

    /// Gives the complete part file its final name.
    async fn save(&self, part: &FsPath, size: u64) -> io::Result<Outcome> {
        let (part, dir, name) = (
            part.to_path_buf(),
            self.dir.to_path_buf(),
            self.name.clone(),
        );
        let path = tokio::task::spawn_blocking(move || store::place_file(&part, &dir, &name))
            .await
            .map_err(io::Error::other)??;
        Ok(Outcome::Saved { path, size })
    }

    fn respond(&self, result: Result<Outcome, Failure>, transfer: &mut Transfer) -> Response {
        match result {
            Ok(Outcome::Saved { path, size }) => {
                transfer.finish();
                let name = file_name(&path);
                self.state.manager.touch(&self.share.id);
                (self.state.sink)(ShareEvent::Received {
                    id: self.share.id.clone(),
                    name: name.clone(),
                    size,
                    kind: FileKind::from_mime(mime_for(&path)),
                    path,
                });
                log::info!("Datei vom Handy empfangen ({size} Bytes)");
                json(StatusCode::CREATED, serde_json::json!({ "name": name }))
            }
            Ok(Outcome::AlreadySaved { name }) => {
                json(StatusCode::CREATED, serde_json::json!({ "name": name }))
            }
            Ok(Outcome::Partial { offset }) => json(
                StatusCode::ACCEPTED,
                serde_json::json!({ "offset": offset }),
            ),
            Err(Failure::Offset(offset)) => json(
                StatusCode::CONFLICT,
                serde_json::json!({ "error": "offset", "offset": offset }),
            ),
            Err(Failure::TooManyUnfinished) => {
                json_error(StatusCode::TOO_MANY_REQUESTS, "too_many")
            }
            Err(Failure::Superseded) => json_error(StatusCode::CONFLICT, "superseded"),
            Err(Failure::Cancelled) => {
                self.broke_off(transfer, json_error(StatusCode::GONE, "cancelled"))
            }
            Err(Failure::Incomplete) => {
                self.broke_off(transfer, json_error(StatusCode::BAD_REQUEST, "incomplete"))
            }
            Err(Failure::Oversized) => {
                self.broke_off(transfer, json_error(StatusCode::BAD_REQUEST, "size"))
            }
            Err(Failure::Io(e)) => {
                log::warn!("Empfang unterbrochen: {}", e.kind());
                let response = match e.kind() {
                    io::ErrorKind::StorageFull => {
                        json_error(StatusCode::INSUFFICIENT_STORAGE, "disk_full")
                    }
                    io::ErrorKind::FileTooLarge => {
                        json_error(StatusCode::PAYLOAD_TOO_LARGE, "too_large")
                    }
                    io::ErrorKind::ConnectionAborted => {
                        json_error(StatusCode::BAD_REQUEST, "aborted")
                    }
                    _ => json_error(StatusCode::INTERNAL_SERVER_ERROR, "write"),
                };
                self.broke_off(transfer, response)
            }
        }
    }

    /// The upload broke off: tells the window (unless another transfer of
    /// the session is still running) and answers with `response`.
    fn broke_off(&self, transfer: &mut Transfer, response: Response) -> Response {
        if transfer.abort() {
            (self.state.sink)(ShareEvent::Interrupted {
                id: self.share.id.clone(),
            });
        }
        response
    }
}

fn file_name(path: &FsPath) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// `POST /s/{token}/text`: a text from the phone goes into the clipboard.
pub(super) async fn text(
    State(state): State<AppState>,
    Path(token): Path<String>,
    body: Body,
) -> Response {
    let Some((share, _)) = session(&state, &token) else {
        return gone();
    };
    connect(&state, &share);
    let bytes = match read_limited(body, MAX_TEXT_BYTES).await {
        Ok(bytes) => bytes,
        Err(e) => return e.into_response(),
    };
    let Ok(text) = String::from_utf8(bytes) else {
        return json_error(StatusCode::BAD_REQUEST, "encoding");
    };
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "empty");
    }
    state.manager.touch(&share.id);
    if !(state.text_sink)(&text) {
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, "clipboard");
    }
    (state.sink)(ShareEvent::TextReceived {
        id: share.id.clone(),
        chars: text.chars().count(),
        preview: (!looks_like_secret(trimmed)).then(|| text_preview(&text, 80)),
    });
    // Never log the text itself.
    log::info!("Text vom Handy in die Zwischenablage gelegt");
    StatusCode::NO_CONTENT.into_response()
}

/// Why [`read_limited`] gave up. A small enum instead of a ready `Response`,
/// which is too large for an `Err` (clippy `result_large_err`).
#[derive(Debug)]
enum BodyError {
    Aborted,
    TooLarge,
}

impl IntoResponse for BodyError {
    fn into_response(self) -> Response {
        match self {
            Self::Aborted => json_error(StatusCode::BAD_REQUEST, "aborted"),
            Self::TooLarge => json_error(StatusCode::PAYLOAD_TOO_LARGE, "too_large"),
        }
    }
}

/// Reads a small body completely, failing with 413 (as JSON) once it exceeds
/// `limit` (axum's own limit would answer in plain text). An oversized body
/// is still read on (and dropped) up to [`TEXT_DRAIN_FACTOR`] × `limit`:
/// closing a connection with unread data resets it, and the phone would see
/// a network error instead of the answer.
async fn read_limited(body: Body, limit: usize) -> Result<Vec<u8>, BodyError> {
    let mut stream = body.into_data_stream();
    let mut out = Vec::new();
    let mut seen = 0usize;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| BodyError::Aborted)?;
        seen = seen.saturating_add(chunk.len());
        if seen > limit.saturating_mul(TEXT_DRAIN_FACTOR) {
            break;
        }
        if seen <= limit {
            out.extend_from_slice(&chunk);
        }
    }
    if seen > limit {
        return Err(BodyError::TooLarge);
    }
    Ok(out)
}
