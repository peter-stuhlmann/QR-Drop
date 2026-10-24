//! Routes, all under a secret token: landing page, status, text pickup and
//! image preview. Downloads live in [`super::download`], uploads in
//! [`super::receive`].

use std::path::Path as FsPath;
use std::time::Instant;

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use bytes::Bytes;
use serde::Serialize;

use super::download::{self, DOWNLOAD_CSP};
use super::page::{LANDING, UPLOAD};
use super::{json, not_found, receive, AppState};
use crate::share::{mime_for, ActiveShare, FileKind, Lookup, ShareEvent, ShareSource};

/// Previews only for images up to 50 MB (CLAUDE.md §5.1).
const PREVIEW_MAX_SOURCE: u64 = 50 * 1024 * 1024;
const PREVIEW_EDGE: u32 = 1200;
/// Decoder limits: a small file can claim gigantic dimensions.
const PREVIEW_MAX_ALLOC: u64 = 128 * 1024 * 1024;
const PREVIEW_MAX_DIMENSION: u32 = 16_384;
/// Formats the `image` crate is built with (see Cargo.toml). Other images
/// (HEIC, …) get no preview instead of a broken one.
const PREVIEW_FORMATS: &[&str] = &["image/jpeg", "image/png", "image/webp", "image/gif"];

pub(super) fn router(state: AppState) -> Router {
    Router::new()
        .route("/s/{token}", get(landing))
        .route("/s/{token}/file", get(download::download))
        .route("/s/{token}/preview", get(preview))
        .route("/s/{token}/status", get(status))
        .route("/s/{token}/take", post(take))
        .route("/s/{token}/check", get(receive::check))
        .route(
            "/s/{token}/upload",
            post(receive::upload).get(receive::upload_offset),
        )
        .route("/s/{token}/text", post(receive::text))
        .fallback(|| async { not_found() })
        .with_state(state)
}

/// Data of the download page. The default is the "code no longer valid"
/// page.
#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct LandingData<'a> {
    valid: bool,
    name: &'a str,
    size: u64,
    kind: Option<FileKind>,
    is_zip: bool,
    file_count: usize,
    has_preview: bool,
    expires_in: u64,
    /// A text share: the page fetches it with `POST …/take`.
    is_text: bool,
    secret: bool,
    /// Length of the text, so the page can prepare its layout.
    chars: usize,
}

impl<'a> LandingData<'a> {
    fn valid(share: &'a ActiveShare) -> Self {
        Self {
            valid: true,
            name: share.source.name(),
            size: share.source.size(),
            kind: Some(share.source.kind()),
            is_zip: share.source.is_zip(),
            expires_in: share
                .expires_at
                .saturating_duration_since(Instant::now())
                .as_secs(),
            ..Self::default()
        }
    }
}

/// Data of the upload page (phone → computer).
#[derive(Serialize)]
struct UploadData<'a> {
    valid: bool,
    /// Name of the target folder, e.g. "Downloads".
    folder: &'a str,
}

/// Tells the window that the phone opened the page (once per share).
pub(super) fn connect(state: &AppState, share: &ActiveShare) {
    if state.manager.mark_connected(&share.id) {
        (state.sink)(ShareEvent::Connected {
            id: share.id.clone(),
        });
    }
}

/// `GET /s/{token}`: the page for the phone. Showing it never uses up the
/// share -- link previews, prefetching and reloads would burn it.
async fn landing(State(state): State<AppState>, Path(token): Path<String>) -> Response {
    let Some(share) = state.manager.by_token(&token) else {
        return LANDING.render(StatusCode::NOT_FOUND, &LandingData::default());
    };
    let data = match &*share.source {
        ShareSource::File { .. } => LandingData {
            file_count: 1,
            has_preview: has_preview(&share),
            ..LandingData::valid(&share)
        },
        ShareSource::Zip { file_count, .. } => LandingData {
            file_count: *file_count,
            ..LandingData::valid(&share)
        },
        ShareSource::Text { text, secret } => LandingData {
            is_text: true,
            secret: *secret,
            chars: text.chars().count(),
            ..LandingData::valid(&share)
        },
        ShareSource::Receive { .. } => {
            connect(&state, &share);
            let data = UploadData {
                valid: true,
                folder: share.source.name(),
            };
            return UPLOAD.render(StatusCode::OK, &data);
        }
        // Links are encoded directly in the QR code; nothing to serve.
        ShareSource::Url { .. } => return not_found(),
    };
    connect(&state, &share);
    LANDING.render(StatusCode::OK, &data)
}

/// `POST /s/{token}/take`: hands out a shared text. This, not the page, is
/// the single "download" of a text share.
async fn take(State(state): State<AppState>, Path(token): Path<String>) -> Response {
    let Some(share) = state.manager.by_token(&token) else {
        return not_found();
    };
    let ShareSource::Text { text, .. } = &*share.source else {
        return not_found();
    };
    // Decides atomically: of two racing requests only one gets the text.
    if !state.manager.record_download(&share.id) {
        return not_found();
    }
    connect(&state, &share);
    (state.sink)(ShareEvent::Done {
        id: share.id.clone(),
    });
    json(StatusCode::OK, serde_json::json!({ "text": text }))
}

/// `GET /s/{token}/status`: lets the page notice the end of a share without
/// counting as a visit. 204 while valid, otherwise 404 with the reason.
async fn status(State(state): State<AppState>, Path(token): Path<String>) -> Response {
    let reason = match state.manager.lookup(&token) {
        Lookup::Valid(_) => return StatusCode::NO_CONTENT.into_response(),
        Lookup::Done { .. } => "done",
        Lookup::Expired => "expired",
        Lookup::Unknown => "unknown",
    };
    json(
        StatusCode::NOT_FOUND,
        serde_json::json!({ "reason": reason }),
    )
}

fn has_preview(share: &ActiveShare) -> bool {
    matches!(
        &*share.source,
        ShareSource::File { path, size, .. }
            if *size <= PREVIEW_MAX_SOURCE && PREVIEW_FORMATS.contains(&mime_for(path))
    )
}

/// `GET /s/{token}/preview`: a small JPEG of a shared image, computed once.
async fn preview(State(state): State<AppState>, Path(token): Path<String>) -> Response {
    let Some(share) = state.manager.by_token(&token) else {
        return not_found();
    };
    let ShareSource::File { path, .. } = &*share.source else {
        return not_found();
    };
    if !has_preview(&share) {
        return not_found();
    }
    let path = path.clone();
    let cached = share
        .preview
        .get_or_init(|| async move {
            // A join error (the decoder panicked) can only occur in debug
            // builds; release builds abort on panic, which the decoder
            // limits are there to prevent.
            tokio::task::spawn_blocking(move || make_preview(&path).ok())
                .await
                .ok()
                .flatten()
        })
        .await;
    let Some(jpeg) = cached else {
        return not_found();
    };
    let mut res = Response::new(Body::from(jpeg.clone()));
    let h = res.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static("image/jpeg"));
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(DOWNLOAD_CSP),
    );
    res
}

fn make_preview(path: &FsPath) -> image::ImageResult<Bytes> {
    use image::ImageDecoder;
    let mut reader = image::ImageReader::open(path)?.with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(PREVIEW_MAX_ALLOC);
    limits.max_image_width = Some(PREVIEW_MAX_DIMENSION);
    limits.max_image_height = Some(PREVIEW_MAX_DIMENSION);
    reader.limits(limits);
    let mut decoder = reader.into_decoder()?;
    let orientation = decoder.orientation()?;
    let mut img = image::DynamicImage::from_decoder(decoder)?;
    img.apply_orientation(orientation);
    let thumb = img.thumbnail(PREVIEW_EDGE, PREVIEW_EDGE).to_rgb8();
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 80).encode_image(&thumb)?;
    Ok(Bytes::from(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oversized_images_are_refused_by_the_decoder() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("huge.png");
        // 20000 × 1 pixels: tiny file, but beyond the dimension limit.
        image::GrayImage::new(20_000, 1).save(&path).unwrap();
        assert!(make_preview(&path).is_err());
        let ok = dir.path().join("ok.png");
        image::GrayImage::new(10, 10).save(&ok).unwrap();
        assert!(make_preview(&ok).is_ok());
    }
}
