//! Download helpers: Range parsing, validators, Content-Disposition and
//! progress tracking.

use std::ops::Range;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::http::{header, HeaderMap, HeaderValue};
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};

use super::AppState;
use crate::share::{ActiveShare, EventSink, ShareEvent, Transfer, TransferKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ByteRange {
    pub start: u64,
    /// Inclusive.
    pub end: u64,
}

impl ByteRange {
    /// Number of bytes; never zero because `end` is inclusive.
    pub fn byte_count(&self) -> u64 {
        self.end - self.start + 1
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum RangeResult {
    /// No (usable) Range header: send everything.
    Full,
    Partial(ByteRange),
    /// 416 Range Not Satisfiable.
    Unsatisfiable,
}

/// Parses a single `bytes=` range (RFC 9110 §14.1.2). Multiple ranges are
/// answered with the full body, which every client must accept; so is any
/// range of an empty file, which has no satisfiable range at all.
pub(super) fn parse_range(header: Option<&str>, size: u64) -> RangeResult {
    let Some(value) = header.map(str::trim) else {
        return RangeResult::Full;
    };
    // The unit is case-insensitive.
    let Some(spec) = value
        .get(..6)
        .filter(|unit| unit.eq_ignore_ascii_case("bytes="))
        .map(|_| &value[6..])
    else {
        return RangeResult::Full;
    };
    if spec.contains(',') || size == 0 {
        return RangeResult::Full;
    }
    let Some((start, end)) = spec.trim().split_once('-') else {
        return RangeResult::Full;
    };
    let (start, end) = (start.trim(), end.trim());
    let range = if start.is_empty() {
        // Suffix range: last N bytes.
        let Ok(n) = end.parse::<u64>() else {
            return RangeResult::Full;
        };
        if n == 0 {
            return RangeResult::Unsatisfiable;
        }
        ByteRange {
            start: size.saturating_sub(n),
            end: size - 1,
        }
    } else {
        let Ok(start) = start.parse::<u64>() else {
            return RangeResult::Full;
        };
        let end = if end.is_empty() {
            size - 1
        } else {
            match end.parse::<u64>() {
                Ok(e) => e.min(size - 1),
                Err(_) => return RangeResult::Full,
            }
        };
        if start >= size || start > end {
            return RangeResult::Unsatisfiable;
        }
        ByteRange { start, end }
    };
    RangeResult::Partial(range)
}

/// Identifies the version of a shared file, so a resumed download never
/// glues together bytes of two different versions.
pub(super) struct Validators {
    /// Weak ETag from modification time and size: cheap and good enough to
    /// notice a changed file, but no byte-for-byte guarantee.
    etag: String,
    last_modified: Option<String>,
}

impl Validators {
    pub fn of(meta: &std::fs::Metadata) -> Self {
        let modified = meta.modified().ok();
        let stamp = modified
            .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
            .unwrap_or_default();
        Self {
            etag: format!(
                "W/\"{:x}.{:x}-{:x}\"",
                stamp.as_secs(),
                stamp.subsec_nanos(),
                meta.len()
            ),
            last_modified: modified.map(http_date),
        }
    }

    /// `If-Range` (RFC 9110 §13.1.5): a range only applies to the version
    /// the client already has. Entity tags need a strong match, which a weak
    /// tag never is, so only the exact Last-Modified date matches.
    pub fn range_applies(&self, if_range: Option<&str>) -> bool {
        match if_range.map(str::trim) {
            None => true,
            Some(tag) if tag.starts_with('"') || tag.starts_with("W/") => false,
            Some(date) => self.last_modified.as_deref() == Some(date),
        }
    }

    pub fn insert_into(&self, headers: &mut HeaderMap) {
        if let Ok(v) = HeaderValue::from_str(&self.etag) {
            headers.insert(header::ETAG, v);
        }
        if let Some(v) = self
            .last_modified
            .as_deref()
            .and_then(|d| HeaderValue::from_str(d).ok())
        {
            headers.insert(header::LAST_MODIFIED, v);
        }
    }
}

/// IMF-fixdate, e.g. `Sun, 06 Nov 1994 08:49:37 GMT`.
fn http_date(time: SystemTime) -> String {
    chrono::DateTime::<chrono::Utc>::from(time)
        .format("%a, %d %b %Y %H:%M:%S GMT")
        .to_string()
}

/// RFC 5987 `attr-char`: everything else gets percent-encoded.
const ATTR_CHAR: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'!')
    .remove(b'#')
    .remove(b'$')
    .remove(b'&')
    .remove(b'+')
    .remove(b'-')
    .remove(b'.')
    .remove(b'^')
    .remove(b'_')
    .remove(b'`')
    .remove(b'|')
    .remove(b'~');

/// `attachment; filename="…"; filename*=UTF-8''…` (RFC 6266 / RFC 5987).
pub(super) fn content_disposition(name: &str) -> String {
    let ascii: String = name
        .chars()
        .map(|c| match c {
            '"' | '\\' | '/' => '_',
            c if c.is_ascii() && !c.is_ascii_control() => c,
            _ => '_',
        })
        .collect();
    let ascii = truncate_keep_extension(&ascii, 150);
    let encoded = utf8_percent_encode(name, ATTR_CHAR).to_string();
    format!("attachment; filename=\"{ascii}\"; filename*=UTF-8''{encoded}")
}

/// Only used on the ASCII fallback name, so byte slicing is safe.
fn truncate_keep_extension(name: &str, max: usize) -> String {
    if name.len() <= max {
        return name.to_string();
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if e.len() < 12 => (s, format!(".{e}")),
        _ => (name, String::new()),
    };
    let keep = max.saturating_sub(ext.len());
    format!("{}{ext}", &stem[..keep.min(stem.len())])
}

/// Reports one download to the UI: progress (shared per share, at most 10×
/// per second), `Done` once the phone has everything, `Interrupted` if it
/// broke off while no other download of the share is running.
pub(super) struct DownloadTracker {
    transfer: Transfer,
    sink: EventSink,
    /// File size, or uncompressed total of a ZIP.
    total: u64,
    /// The share was reported as delivered by this download.
    recorded: bool,
    /// The body went out completely.
    finished: bool,
}

impl DownloadTracker {
    pub fn new(state: &AppState, share: &ActiveShare, kind: TransferKind, total: u64) -> Self {
        Self {
            transfer: state.manager.begin_transfer(&share.id, kind),
            sink: state.sink.clone(),
            total,
            recorded: false,
            finished: false,
        }
    }

    /// True once the share was closed or another download delivered it.
    pub fn is_cancelled(&self) -> bool {
        self.transfer.is_cancelled()
    }

    /// File downloads: `range` of the file was handed to the phone. Ranges
    /// from all requests add up, so range-only clients complete the share.
    pub fn delivered(&mut self, range: Range<u64>) {
        let (covered, completed) =
            self.transfer
                .manager()
                .add_delivered(self.transfer.id(), range, self.total);
        if completed {
            self.record();
        } else {
            self.report(covered, false);
        }
    }

    /// ZIP downloads: source bytes packed so far.
    pub fn packed(&mut self, bytes: u64) {
        self.report(bytes, false);
    }

    /// The body went out completely. `whole`: it was the complete file or
    /// archive, which delivers the share by itself.
    pub fn finish(&mut self, whole: bool) {
        if whole {
            self.record();
        }
        self.finished = true;
        self.transfer.finish();
    }

    fn record(&mut self) {
        if std::mem::replace(&mut self.recorded, true) {
            return;
        }
        self.report(self.total, true);
        if self.transfer.manager().record_download(self.transfer.id()) {
            self.transfer.delivered_share();
            (self.sink)(ShareEvent::Done {
                id: self.transfer.id().to_string(),
            });
        }
    }

    fn report(&self, sent: u64, force: bool) {
        let id = self.transfer.id();
        if let Some(rate) = self.transfer.manager().progress_sample(id, sent, force) {
            (self.sink)(ShareEvent::Progress {
                id: id.to_string(),
                sent: sent.min(self.total),
                total: self.total,
                bytes_per_sec: rate,
            });
        }
    }
}

impl Drop for DownloadTracker {
    fn drop(&mut self) {
        if !self.finished && self.transfer.abort() {
            (self.sink)(ShareEvent::Interrupted {
                id: self.transfer.id().to_string(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    use crate::server::receive::Uploads;
    use crate::share::{new_id, ShareConfig, ShareManager, ShareSource};

    fn r(start: u64, end: u64) -> RangeResult {
        RangeResult::Partial(ByteRange { start, end })
    }

    #[test]
    fn range_parsing() {
        assert_eq!(parse_range(None, 100), RangeResult::Full);
        assert_eq!(parse_range(Some("bytes=0-"), 100), r(0, 99));
        assert_eq!(parse_range(Some("Bytes=0-0"), 100), r(0, 0));
        assert_eq!(parse_range(Some("bytes=10-19"), 100), r(10, 19));
        assert_eq!(parse_range(Some("bytes=90-500"), 100), r(90, 99));
        assert_eq!(parse_range(Some("bytes=-10"), 100), r(90, 99));
        assert_eq!(parse_range(Some("bytes=-500"), 100), r(0, 99));
        assert_eq!(
            parse_range(Some("bytes=100-"), 100),
            RangeResult::Unsatisfiable
        );
        assert_eq!(
            parse_range(Some("bytes=50-10"), 100),
            RangeResult::Unsatisfiable
        );
        assert_eq!(
            parse_range(Some("bytes=-0"), 100),
            RangeResult::Unsatisfiable
        );
        assert_eq!(parse_range(Some("bytes=0-1,5-6"), 100), RangeResult::Full);
        assert_eq!(parse_range(Some("items=0-1"), 100), RangeResult::Full);
        assert_eq!(parse_range(Some("bytes"), 100), RangeResult::Full);
        assert_eq!(parse_range(Some("bytes=abc-"), 100), RangeResult::Full);
        // An empty file is simply sent (empty) in full.
        assert_eq!(parse_range(Some("bytes=0-"), 0), RangeResult::Full);
        // > 4 GB offsets.
        let big = 5 * 1024 * 1024 * 1024u64;
        assert_eq!(
            parse_range(Some("bytes=4294967296-"), big),
            r(4_294_967_296, big - 1)
        );
    }

    #[test]
    fn if_range_needs_the_exact_date() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.bin");
        std::fs::write(&path, b"abc").unwrap();
        let v = Validators::of(&std::fs::metadata(&path).unwrap());
        assert!(v.etag.starts_with("W/\"") && v.etag.ends_with("-3\""));
        let date = v.last_modified.clone().unwrap();
        assert!(date.ends_with(" GMT"), "{date}");
        assert!(v.range_applies(None));
        assert!(v.range_applies(Some(&date)));
        assert!(!v.range_applies(Some("Sun, 06 Nov 1994 08:49:37 GMT")));
        assert!(!v.range_applies(Some(&v.etag)));
        assert!(!v.range_applies(Some("\"strong\"")));
    }

    #[test]
    fn disposition_plain_ascii() {
        assert_eq!(
            content_disposition("report.pdf"),
            "attachment; filename=\"report.pdf\"; filename*=UTF-8''report.pdf"
        );
    }

    #[test]
    fn disposition_umlauts_emoji_and_specials() {
        let d = content_disposition("Grüße \"Ü\" 🎉.jpg");
        assert!(d.starts_with("attachment; filename=\"Gr__e ___ _.jpg\""));
        assert!(d.ends_with("filename*=UTF-8''Gr%C3%BC%C3%9Fe%20%22%C3%9C%22%20%F0%9F%8E%89.jpg"));
        let d = content_disposition("a\\b/c;d.txt");
        assert!(d.contains("filename=\"a_b_c;d.txt\""));
        assert!(d.contains("UTF-8''a%5Cb%2Fc%3Bd.txt"));
    }

    #[test]
    fn disposition_very_long_name_keeps_extension() {
        let name = format!("{}.mp4", "x".repeat(400));
        let d = content_disposition(&name);
        let ascii = d.split('"').nth(1).unwrap();
        assert_eq!(ascii.len(), 150);
        assert!(ascii.ends_with(".mp4"));
    }

    #[test]
    fn tracker_reports_done_once_and_interrupt_otherwise() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let ev = events.clone();
        let state = AppState {
            manager: Arc::new(ShareManager::new(ShareConfig::default())),
            sink: Arc::new(move |e| ev.lock().unwrap().push(e)),
            text_sink: Arc::new(|_: &str| true),
            uploads: Arc::new(Uploads::default()),
        };
        let share = state
            .manager
            .create_with_id(
                new_id().unwrap(),
                ShareSource::File {
                    path: "x".into(),
                    name: "x".into(),
                    size: 10,
                },
            )
            .unwrap();

        // An interrupted download is reported while the share is open.
        let mut t = DownloadTracker::new(&state, &share, TransferKind::Full, 10);
        t.delivered(0..4);
        drop(t);
        assert!(matches!(
            events.lock().unwrap().last(),
            Some(ShareEvent::Interrupted { .. })
        ));

        // A resumed range completes it: Done exactly once, no interruption.
        events.lock().unwrap().clear();
        let mut t = DownloadTracker::new(&state, &share, TransferKind::Partial, 10);
        t.delivered(4..10);
        t.finish(false);
        drop(t);
        let got = events.lock().unwrap().clone();
        assert_eq!(
            got.iter()
                .filter(|e| matches!(e, ShareEvent::Done { .. }))
                .count(),
            1
        );
        assert!(matches!(
            got.iter().rev().nth(1),
            Some(ShareEvent::Progress {
                sent: 10,
                total: 10,
                ..
            })
        ));
        assert!(!got
            .iter()
            .any(|e| matches!(e, ShareEvent::Interrupted { .. })));
    }
}
