//! Command-line arguments of a start or a second instance, and the debounced
//! collector that merges a multi-selection.
//!
//! Explorer starts one process per selected file; everything that arrives
//! within [`DEBOUNCE`] becomes one share (CLAUDE.md §4.1).

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

const DEBOUNCE: Duration = Duration::from_millis(300);

/// Start menu entry "Zwischenablage senden".
const CLIPBOARD_FLAG: &str = "--clipboard";
/// Start menu entry "Vom Handy empfangen".
const RECEIVE_FLAG: &str = "--receive";

/// What a process start asks QR Drop to do.
#[derive(Debug, PartialEq, Eq)]
pub enum Request {
    /// Share the clipboard text.
    Clipboard,
    /// Receive files and texts from the phone.
    Receive,
    /// Share files or folders (context menu, Quick Action).
    Paths(Vec<PathBuf>),
    /// Nothing usable: show the drop zone (or the welcome window).
    Idle,
}

/// Interprets the arguments after the program name. `cwd` resolves relative
/// paths (a second instance reports its own working directory).
///
/// Arguments are `OsStr`: file names need not be valid Unicode, and a lossy
/// conversion would point at a different file.
pub fn parse_request<I, S>(args: I, cwd: Option<&Path>) -> Request
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args: Vec<S> = args.into_iter().collect();
    let has_flag = |flag: &str| args.iter().any(|a| a.as_ref() == OsStr::new(flag));
    if has_flag(CLIPBOARD_FLAG) {
        return Request::Clipboard;
    }
    if has_flag(RECEIVE_FLAG) {
        return Request::Receive;
    }
    let paths = paths_from_args(&args, cwd);
    if paths.is_empty() {
        Request::Idle
    } else {
        Request::Paths(paths)
    }
}

/// Turns raw arguments into existing, de-duplicated paths. Paths are taken
/// verbatim: leading or trailing spaces are legal in file names.
pub fn paths_from_args<I, S>(args: I, cwd: Option<&Path>) -> Vec<PathBuf>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut out: Vec<PathBuf> = Vec::new();
    for arg in args {
        let arg = arg.as_ref();
        // Flags and macOS process serial numbers ("-psn_0_12345").
        if arg.is_empty() || arg.as_encoded_bytes().starts_with(b"-") {
            continue;
        }
        let path = match cwd {
            Some(cwd) if Path::new(arg).is_relative() => cwd.join(arg),
            _ => PathBuf::from(arg),
        };
        if path.exists() && !out.contains(&path) {
            out.push(path);
        }
    }
    out
}

/// One debounced push: which selection it belongs to and whether it started
/// that selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Push {
    /// Identifies the selection; the flush callback receives the same id.
    pub batch: u64,
    /// Nothing was pending: the caller may show these paths right away.
    pub first: bool,
}

/// Debounced path collector.
#[derive(Default)]
pub struct Collector {
    pending: Mutex<Vec<PathBuf>>,
    /// Bumped on every push; only the timer of the latest push flushes.
    generation: AtomicU64,
    /// Id of the selection currently being collected.
    batch: AtomicU64,
}

impl Collector {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn pending(&self) -> MutexGuard<'_, Vec<PathBuf>> {
        self.pending.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Adds paths and calls `flush(batch, all_paths)` once no new paths
    /// arrived for [`DEBOUNCE`]. Returns `None` for an empty push. Must be
    /// called inside a Tokio runtime.
    pub fn push<F>(self: &Arc<Self>, paths: Vec<PathBuf>, flush: F) -> Option<Push>
    where
        F: FnOnce(u64, Vec<PathBuf>) + Send + 'static,
    {
        if paths.is_empty() {
            return None;
        }
        let push = {
            let mut pending = self.pending();
            let first = pending.is_empty();
            let batch = if first {
                self.batch.fetch_add(1, Ordering::SeqCst) + 1
            } else {
                self.batch.load(Ordering::SeqCst)
            };
            for p in paths {
                if !pending.contains(&p) {
                    pending.push(p);
                }
            }
            Push { batch, first }
        };
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let me = self.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(DEBOUNCE).await;
            if me.generation.load(Ordering::SeqCst) != generation {
                return; // a newer push will flush
            }
            let paths = std::mem::take(&mut *me.pending());
            if !paths.is_empty() {
                flush(push.batch, paths);
            }
        });
        Some(push)
    }

    pub fn has_pending(&self) -> bool {
        !self.pending().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    #[test]
    fn filters_flags_and_missing_paths() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, b"a").unwrap();
        let args = vec![
            OsString::from("-psn_0_1234"),
            OsString::from("--flag"),
            file.clone().into_os_string(),
            file.clone().into_os_string(),
            dir.path().join("missing.txt").into_os_string(),
            OsString::from("a.txt"),
        ];
        let paths = paths_from_args(args, Some(dir.path()));
        assert_eq!(paths, vec![file.clone()]);
    }

    #[test]
    fn keeps_spaces_in_file_names() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(" leading space.txt");
        std::fs::write(&file, b"a").unwrap();
        assert_eq!(
            paths_from_args([" leading space.txt"], Some(dir.path())),
            vec![file]
        );
    }

    #[test]
    fn file_urls_are_not_paths() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, b"a").unwrap();
        let url = format!("file://{}", file.display());
        assert!(paths_from_args([url], None).is_empty());
    }

    #[cfg(windows)]
    fn non_unicode_arg() -> OsString {
        use std::os::windows::ffi::OsStringExt;
        // A lone surrogate is a valid Windows file name but not Unicode.
        OsString::from_wide(&[0x61, 0xD800, 0x62])
    }

    #[cfg(unix)]
    fn non_unicode_arg() -> OsString {
        use std::os::unix::ffi::OsStringExt;
        OsString::from_vec(vec![b'a', 0xFF, b'b'])
    }

    #[test]
    fn non_unicode_arguments_do_not_panic() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            parse_request([non_unicode_arg()], Some(dir.path())),
            Request::Idle
        );
    }

    #[test]
    fn requests() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("foto.jpg"), b"a").unwrap();
        assert_eq!(parse_request(["--clipboard"], None), Request::Clipboard);
        assert_eq!(parse_request(["x", "--receive"], None), Request::Receive);
        assert_eq!(parse_request::<_, &str>([], None), Request::Idle);
        assert_eq!(
            parse_request(["foto.jpg"], Some(dir.path())),
            Request::Paths(vec![dir.path().join("foto.jpg")])
        );
    }

    #[tokio::test]
    async fn debounce_merges_quick_pushes() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        let collector = Collector::new();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let tx2 = tx.clone();
        let first = collector
            .push(vec![a.clone()], move |batch, p| {
                let _ = tx.send((batch, p));
            })
            .unwrap();
        assert!(first.first);
        tokio::time::sleep(Duration::from_millis(100)).await;
        let second = collector
            .push(vec![b.clone(), a.clone()], move |batch, p| {
                let _ = tx2.send((batch, p));
            })
            .unwrap();
        assert_eq!(
            second,
            Push {
                batch: first.batch,
                first: false
            }
        );
        let got = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(got, (first.batch, vec![a.clone(), b]));
        // Only one flush: either nothing arrives or the channel is closed.
        let again = tokio::time::timeout(Duration::from_millis(500), rx.recv()).await;
        assert!(matches!(again, Err(_) | Ok(None)));
        assert!(!collector.has_pending());

        // The next selection gets a new id.
        let next = collector.push(vec![a], |_, _| {}).unwrap();
        assert!(next.first);
        assert_ne!(next.batch, first.batch);
        assert!(collector.push(Vec::new(), |_, _| {}).is_none());
    }
}
