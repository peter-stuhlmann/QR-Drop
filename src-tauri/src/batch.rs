//! Multi-selections: the first paths of a selection are shown at once, the
//! rest is merged in after the debounce (one process per file on Windows,
//! CLAUDE.md §4.1 and §12).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use tauri::AppHandle;

use crate::app_core;
use crate::handoff::{self, Handoff};
use crate::sessions;
use crate::ui;

/// Selections whose first window and debounce end are still being matched
/// up, by batch id (see [`crate::args::Push`]). Opening the first window
/// and the end of the debounce can happen in either order: on a cold start
/// the window only appears once the event loop runs, possibly after the
/// debounce. Keyed by id, so a second selection never overwrites the first.
#[derive(Debug, Default)]
pub struct Batches(Mutex<HashMap<u64, Batch>>);

#[derive(Debug, Default)]
struct Batch {
    /// Every path of the first push (hand-offs included).
    first_paths: Vec<PathBuf>,
    /// The ordinary files among them, shown in the first window.
    first_files: Vec<PathBuf>,
    /// `Some(share_id)` once the first window is open (`Some(None)`: no
    /// file window, e.g. only a text hand-off).
    opened: Option<Option<String>>,
    /// The whole selection, if the debounce ended before the window opened.
    complete: Option<Vec<PathBuf>>,
}

/// A finished selection: the share shown first and what arrived on top.
#[derive(Debug, PartialEq, Eq)]
pub struct BatchDone {
    pub share_id: Option<String>,
    pub first_files: Vec<PathBuf>,
    /// Paths of the selection that were not part of the first push.
    pub new_paths: Vec<PathBuf>,
}

impl Batch {
    fn finish(self, share_id: Option<String>, all: Vec<PathBuf>) -> BatchDone {
        let new_paths = all
            .into_iter()
            .filter(|p| !self.first_paths.contains(p))
            .collect();
        BatchDone {
            share_id,
            first_files: self.first_files,
            new_paths,
        }
    }
}

impl Batches {
    fn lock(&self) -> MutexGuard<'_, HashMap<u64, Batch>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Registers the first push of a selection.
    pub fn begin(&self, id: u64, first_paths: Vec<PathBuf>, first_files: Vec<PathBuf>) {
        self.lock().insert(
            id,
            Batch {
                first_paths,
                first_files,
                ..Batch::default()
            },
        );
    }

    /// Whether a selection is still waiting for its first window or the end
    /// of its debounce.
    pub fn is_busy(&self) -> bool {
        !self.lock().is_empty()
    }

    /// The first window of selection `id` is open. Returns the selection if
    /// its debounce already ended.
    pub fn opened(&self, id: u64, share_id: Option<String>) -> Option<BatchDone> {
        let mut batches = self.lock();
        let batch = batches.get_mut(&id)?;
        match batch.complete.take() {
            Some(all) => batches.remove(&id).map(|b| b.finish(share_id, all)),
            None => {
                batch.opened = Some(share_id);
                None
            }
        }
    }

    /// The debounce of selection `id` ended. Returns the selection once its
    /// first window is open; otherwise [`opened`](Self::opened) will.
    pub fn complete(&self, id: u64, all: Vec<PathBuf>) -> Option<BatchDone> {
        let mut batches = self.lock();
        let Some(batch) = batches.get_mut(&id) else {
            // Never begun: nothing was shown yet, everything is new.
            return Some(Batch::default().finish(None, all));
        };
        match batch.opened.take() {
            Some(share_id) => batches.remove(&id).map(|b| b.finish(share_id, all)),
            None => {
                batch.complete = Some(all);
                None
            }
        }
    }
}

/// Paths from the OS (context menu, Quick Action, Dock, second instance):
/// the first paths of a selection open a window right away, later ones are
/// merged in once the debounce ends.
pub fn queue_paths(app: &AppHandle, paths: Vec<PathBuf>) {
    let Some(core) = app_core::get(app) else {
        return;
    };
    let handle = app.clone();
    let push = core
        .collector
        .push(paths.clone(), move |batch, all| finish(&handle, batch, all));
    let Some(push) = push.filter(|p| p.first) else {
        return;
    };
    let (files, handoffs) = handoff::split(&paths);
    // Registered synchronously, long before the debounce (300 ms) can end.
    core.batches.begin(push.batch, paths, files.clone());
    let handle = app.clone();
    tauri::async_runtime::spawn(async move { show_first(&handle, push.batch, files, handoffs) });
}

/// Opens the first window of a selection.
fn show_first(app: &AppHandle, batch: u64, files: Vec<PathBuf>, handoffs: Vec<Handoff>) {
    run_handoffs(app, handoffs);
    let share_id = sessions::start_share(app, files);
    if let Some(done) = app_core::get(app).and_then(|core| core.batches.opened(batch, share_id)) {
        apply(app, done);
    }
    ui::exit_if_idle(app);
}

/// The complete selection after the debounce.
fn finish(app: &AppHandle, batch: u64, all: Vec<PathBuf>) {
    if let Some(done) = app_core::get(app).and_then(|core| core.batches.complete(batch, all)) {
        apply(app, done);
    }
    ui::exit_if_idle(app);
}

/// If more files arrived than were shown first, the first share grows into
/// a ZIP of all of them.
fn apply(app: &AppHandle, done: BatchDone) {
    if done.new_paths.is_empty() {
        return;
    }
    let (new_files, handoffs) = handoff::split(&done.new_paths);
    run_handoffs(app, handoffs);
    if new_files.is_empty() {
        return;
    }
    match done.share_id {
        Some(id) if ui::has_session_window(app, &id) => {
            let all_files = [done.first_files, new_files].concat();
            sessions::grow_share(app, &id, all_files);
        }
        // The first window was closed meanwhile (or there was none).
        _ => {
            sessions::start_share(app, new_files);
        }
    }
}

/// Hand-offs of the macOS services: texts become text shares, the receive
/// request opens "Vom Handy empfangen".
fn run_handoffs(app: &AppHandle, handoffs: Vec<Handoff>) {
    for handoff in handoffs {
        match handoff {
            Handoff::Text(path) => {
                let text = handoff::read_text(&path);
                handoff::discard(&path);
                match text {
                    Some(text) => sessions::start_text_share(app, &text),
                    None => log::warn!("Übergebener Text nicht lesbar"),
                }
            }
            Handoff::Receive(path) => {
                handoff::discard(&path);
                sessions::start_receive(app);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(name: &str) -> PathBuf {
        PathBuf::from(name)
    }

    #[test]
    fn window_first_then_debounce() {
        let batches = Batches::default();
        batches.begin(1, vec![p("a")], vec![p("a")]);
        assert_eq!(batches.opened(1, Some("id".into())), None);
        assert_eq!(
            batches.complete(1, vec![p("a"), p("b")]),
            Some(BatchDone {
                share_id: Some("id".into()),
                first_files: vec![p("a")],
                new_paths: vec![p("b")],
            })
        );
        // Finished batches are gone.
        assert_eq!(batches.opened(1, None), None);
    }

    #[test]
    fn debounce_first_then_window() {
        // Cold start: the event loop is not running yet when the debounce ends.
        let batches = Batches::default();
        batches.begin(1, vec![p("a")], vec![p("a")]);
        assert_eq!(batches.complete(1, vec![p("a"), p("b"), p("c")]), None);
        assert_eq!(
            batches.opened(1, Some("id".into())),
            Some(BatchDone {
                share_id: Some("id".into()),
                first_files: vec![p("a")],
                new_paths: vec![p("b"), p("c")],
            })
        );
    }

    #[test]
    fn single_file_is_done_once() {
        let batches = Batches::default();
        batches.begin(1, vec![p("a")], vec![p("a")]);
        assert_eq!(batches.complete(1, vec![p("a")]), None);
        let done = batches.opened(1, Some("id".into())).unwrap();
        assert!(done.new_paths.is_empty());
    }

    #[test]
    fn a_second_selection_does_not_overwrite_the_first() {
        // Cold start: selection 1 completes before its window opens, then a
        // second right-click starts selection 2.
        let batches = Batches::default();
        batches.begin(1, vec![p("a")], vec![p("a")]);
        assert_eq!(batches.complete(1, vec![p("a"), p("b")]), None);
        batches.begin(2, vec![p("x")], vec![p("x")]);
        assert_eq!(
            batches.opened(1, Some("one".into())),
            Some(BatchDone {
                share_id: Some("one".into()),
                first_files: vec![p("a")],
                new_paths: vec![p("b")],
            })
        );
        assert_eq!(batches.opened(2, Some("two".into())), None);
        assert_eq!(
            batches.complete(2, vec![p("x")]).unwrap().share_id,
            Some("two".into())
        );
    }

    #[test]
    fn hand_offs_are_not_shown_as_files() {
        let batches = Batches::default();
        batches.begin(1, vec![p("t.qrdroptext"), p("a")], vec![p("a")]);
        assert_eq!(batches.opened(1, Some("id".into())), None);
        let done = batches
            .complete(1, vec![p("t.qrdroptext"), p("a"), p("b")])
            .unwrap();
        assert_eq!(done.first_files, vec![p("a")]);
        assert_eq!(done.new_paths, vec![p("b")]);
    }

    #[test]
    fn unknown_batch_is_entirely_new() {
        let batches = Batches::default();
        assert_eq!(
            batches.complete(7, vec![p("a")]),
            Some(BatchDone {
                share_id: None,
                first_files: Vec::new(),
                new_paths: vec![p("a")],
            })
        );
    }

    #[test]
    fn busy_until_window_and_debounce_are_both_done() {
        // Cold start: the debounce ends before the first window exists; the
        // app must not consider itself idle in between.
        let batches = Batches::default();
        assert!(!batches.is_busy());
        batches.begin(1, vec![p("a")], vec![p("a")]);
        assert_eq!(batches.complete(1, vec![p("a")]), None);
        assert!(batches.is_busy());
        assert!(batches.opened(1, Some("id".into())).is_some());
        assert!(!batches.is_busy());
    }
}
