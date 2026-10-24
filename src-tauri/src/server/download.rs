//! `GET /s/{token}/file`: the shared file (with Range support) or the ZIP
//! of a folder / several files, streamed without temp files.

use std::io;
use std::path::Path as FsPath;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use futures_util::StreamExt;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::sync::mpsc;
use tokio_util::io::ReaderStream;

use super::routes::connect;
use super::stream::{content_disposition, parse_range, DownloadTracker, RangeResult, Validators};
use super::zip::{is_client_gone, write_archive, ChannelWriter};
use super::{not_found, AppState, CHUNK};
use crate::share::{mime_for, ActiveShare, Lookup, ShareSource, TransferKind, ZipEntry};

/// Range requests are still answered this long after the single download
/// completed, so the phone can re-request the last bytes.
const RANGE_GRACE: Duration = Duration::from_secs(30);

/// A ZIP whose producer delivers nothing for this long is given up (e.g. a
/// file on a network drive that stopped answering).
const ZIP_STALL_TIMEOUT: Duration = Duration::from_secs(60);

/// Downloads (and previews) are never rendered by the browser as a page:
/// even a shared HTML or SVG file cannot run script under the share's origin.
pub(super) const DOWNLOAD_CSP: &str = "sandbox; default-src 'none'";

pub(super) async fn download(
    State(state): State<AppState>,
    Path(token): Path<String>,
    method: Method,
    headers: HeaderMap,
) -> Response {
    let has_range = headers.contains_key(header::RANGE);
    let (share, parts_only) = match state.manager.lookup(&token) {
        Lookup::Valid(share) => (share, false),
        Lookup::Done { share, at }
            if has_range
                && at.elapsed() < RANGE_GRACE
                && matches!(*share.source, ShareSource::File { .. }) =>
        {
            (share, true)
        }
        _ => return not_found(),
    };
    let head_only = method == Method::HEAD;
    match &*share.source {
        ShareSource::File { path, name, .. } => {
            connect(&state, &share);
            let request = FileRequest {
                headers: &headers,
                head_only,
                parts_only,
            };
            serve_file(&state, &share, path, name, &request)
                .await
                .unwrap_or_else(|e| {
                    log::warn!("Download fehlgeschlagen: {}", e.kind());
                    not_found()
                })
        }
        ShareSource::Zip {
            name,
            entries,
            total,
            ..
        } => {
            connect(&state, &share);
            let body = if head_only {
                Body::empty()
            } else {
                zip_body(&state, &share, entries.clone(), *total)
            };
            download_response(body, name, "application/zip")
        }
        // Texts are fetched by the landing page, links never touch the server.
        ShareSource::Text { .. } | ShareSource::Url { .. } | ShareSource::Receive { .. } => {
            not_found()
        }
    }
}

fn download_response(body: Body, name: &str, mime: &str) -> Response {
    let mut res = Response::new(body);
    let h = res.headers_mut();
    if let Ok(v) = HeaderValue::from_str(mime) {
        h.insert(header::CONTENT_TYPE, v);
    }
    if let Ok(v) = HeaderValue::from_str(&content_disposition(name)) {
        h.insert(header::CONTENT_DISPOSITION, v);
    }
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(DOWNLOAD_CSP),
    );
    res
}

/// How the phone asked for a file.
struct FileRequest<'a> {
    headers: &'a HeaderMap,
    head_only: bool,
    /// The single copy is already out: only parts of the file may still be
    /// fetched (see [`RANGE_GRACE`]), never another complete one.
    parts_only: bool,
}

async fn serve_file(
    state: &AppState,
    share: &ActiveShare,
    path: &FsPath,
    name: &str,
    request: &FileRequest<'_>,
) -> io::Result<Response> {
    let mut file = tokio::fs::File::open(path).await?;
    let meta = file.metadata().await?;
    let size = meta.len();
    let validators = Validators::of(&meta);
    let headers = request.headers;
    let if_range = headers.get(header::IF_RANGE).and_then(|v| v.to_str().ok());
    let range = headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .filter(|_| validators.range_applies(if_range));

    let (status, start, len) = match parse_range(range, size) {
        RangeResult::Full => (StatusCode::OK, 0, size),
        RangeResult::Partial(r) => (StatusCode::PARTIAL_CONTENT, r.start, r.byte_count()),
        RangeResult::Unsatisfiable => {
            let mut res = StatusCode::RANGE_NOT_SATISFIABLE.into_response();
            if let Ok(v) = HeaderValue::from_str(&format!("bytes */{size}")) {
                res.headers_mut().insert(header::CONTENT_RANGE, v);
            }
            return Ok(res);
        }
    };
    if request.parts_only && start == 0 && len == size {
        return Ok(not_found());
    }

    let body = if request.head_only {
        Body::empty()
    } else {
        file.seek(io::SeekFrom::Start(start)).await?;
        file_body(state, share, file, start, len, size)
    };
    let mut res = download_response(body, name, mime_for(path));
    *res.status_mut() = status;
    let h = res.headers_mut();
    validators.insert_into(h);
    h.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    h.insert(header::CONTENT_LENGTH, HeaderValue::from(len));
    if status == StatusCode::PARTIAL_CONTENT {
        let value = format!("bytes {}-{}/{}", start, start + len - 1, size);
        if let Ok(v) = HeaderValue::from_str(&value) {
            h.insert(header::CONTENT_RANGE, v);
        }
    }
    Ok(res)
}

/// Streams `len` bytes from the current position (`start`) of `file`.
/// Stops when the share is closed, or when another complete download
/// already delivered the single copy.
fn file_body(
    state: &AppState,
    share: &ActiveShare,
    file: tokio::fs::File,
    start: u64,
    len: u64,
    size: u64,
) -> Body {
    let whole = start == 0 && len == size;
    let kind = if whole {
        TransferKind::Full
    } else {
        TransferKind::Partial
    };
    let mut tracker = DownloadTracker::new(state, share, kind, size);
    if len == 0 {
        // Empty file: nothing to stream, but it is a complete download.
        tracker.finish(true);
        return Body::empty();
    }
    let end = start + len;
    let mut position = start;
    let stream = ReaderStream::with_capacity(file.take(len), CHUNK).map(move |chunk| {
        let chunk = chunk?;
        if tracker.is_cancelled() {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "share closed"));
        }
        let from = position;
        position += chunk.len() as u64;
        tracker.delivered(from..position);
        if position >= end {
            tracker.finish(whole);
        }
        Ok(chunk)
    });
    Body::from_stream(stream)
}

/// Streams the archive while a blocking task writes it. The response only
/// ends normally if the archive was finished; any failure aborts it, so the
/// phone never keeps a truncated ZIP that looks complete.
fn zip_body(state: &AppState, share: &ActiveShare, entries: Arc<[ZipEntry]>, total: u64) -> Body {
    let packed = Arc::new(AtomicU64::new(0));
    let complete = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel::<Bytes>(8);
    {
        let (packed, complete) = (packed.clone(), complete.clone());
        tokio::task::spawn_blocking(move || {
            match write_archive(&entries, ChannelWriter::new(tx), packed) {
                Ok(writer) => {
                    // Set before the channel closes (dropping `writer`).
                    complete.store(true, Ordering::Release);
                    drop(writer);
                }
                Err(e) if is_client_gone(&e) => {}
                Err(e) => log::warn!("ZIP-Erstellung abgebrochen: {}", e.kind()),
            }
        });
    }

    struct ZipStream {
        rx: mpsc::Receiver<Bytes>,
        tracker: DownloadTracker,
        packed: Arc<AtomicU64>,
        complete: Arc<AtomicBool>,
    }
    let state = ZipStream {
        rx,
        tracker: DownloadTracker::new(state, share, TransferKind::Full, total),
        packed,
        complete,
    };
    let stream = futures_util::stream::unfold(Some(state), |state| async move {
        let mut s = state?;
        if s.tracker.is_cancelled() {
            let e = io::Error::new(io::ErrorKind::Interrupted, "share closed");
            return Some((Err(e), None));
        }
        match tokio::time::timeout(ZIP_STALL_TIMEOUT, s.rx.recv()).await {
            Ok(Some(chunk)) => {
                s.tracker.packed(s.packed.load(Ordering::Relaxed));
                Some((Ok(chunk), Some(s)))
            }
            Ok(None) if s.complete.load(Ordering::Acquire) => {
                s.tracker.finish(true);
                None
            }
            Ok(None) => Some((Err(io::Error::other("archive incomplete")), None)),
            Err(_) => {
                let e = io::Error::new(io::ErrorKind::TimedOut, "archive stalled");
                Some((Err(e), None))
            }
        }
    });
    Body::from_stream(stream)
}
