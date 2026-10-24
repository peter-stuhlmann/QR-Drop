//! Bookkeeping of resumable uploads per receive session.
//!
//! The phone names each file with its own random key; the part file on disk
//! always gets a name chosen here, never one from the phone.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use crate::share::ShareManager;

/// Unfinished uploads kept per session at most (no flooding the disk).
const MAX_PARTS: usize = 64;

/// Finished uploads remembered per session, so a retried request for a file
/// that already arrived is answered without saving it twice.
const MAX_FINISHED: usize = 256;

/// The part file of one keyed upload. Requests take turns: a new request
/// for the same key takes over, and the previous one stops before its next
/// write.
pub(super) struct Part {
    pub path: PathBuf,
    /// Bumped by every request that takes over.
    generation: AtomicU64,
    /// Held for every file operation, so a takeover never interleaves with
    /// the previous writer's last write.
    pub file_lock: tokio::sync::Mutex<()>,
}

/// A request's claim on a part, valid until another request takes over.
pub(super) struct Claim {
    pub part: Arc<Part>,
    generation: u64,
}

impl Claim {
    pub fn is_current(&self) -> bool {
        self.part.generation.load(Ordering::SeqCst) == self.generation
    }
}

/// A file of this session that arrived completely.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Finished {
    /// Name it was saved under.
    pub name: String,
    pub size: u64,
}

#[derive(Default)]
struct Session {
    parts: HashMap<String, Arc<Part>>,
    /// Oldest first; bounded by [`MAX_FINISHED`].
    finished: VecDeque<(String, Finished)>,
}

/// Unfinished and recently finished uploads of all receive sessions.
#[derive(Default)]
pub struct Uploads(Mutex<HashMap<String, Session>>);

impl Uploads {
    fn lock(&self) -> MutexGuard<'_, HashMap<String, Session>> {
        // A poisoned lock only means another thread panicked mid-update.
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The finished upload for `key`, if this session already saved it.
    pub(super) fn finished(&self, session: &str, key: &str) -> Option<Finished> {
        let sessions = self.lock();
        let finished = &sessions.get(session)?.finished;
        finished
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, f)| f.clone())
    }

    /// Part file of an unfinished upload.
    pub(super) fn part_path(&self, session: &str, key: &str) -> Option<PathBuf> {
        let sessions = self.lock();
        let part = sessions.get(session)?.parts.get(key)?;
        Some(part.path.clone())
    }

    /// Takes over the upload `key`; a new key gets the part file `new_path`.
    /// `None` if the session already has too many unfinished uploads.
    pub(super) fn claim(&self, session: &str, key: &str, new_path: PathBuf) -> Option<Claim> {
        let mut sessions = self.lock();
        let parts = &mut sessions.entry(session.to_string()).or_default().parts;
        if !parts.contains_key(key) && parts.len() >= MAX_PARTS {
            return None;
        }
        let part = parts.entry(key.to_string()).or_insert_with(|| {
            Arc::new(Part {
                path: new_path,
                generation: AtomicU64::new(0),
                file_lock: tokio::sync::Mutex::new(()),
            })
        });
        let generation = part.generation.fetch_add(1, Ordering::SeqCst) + 1;
        Some(Claim {
            part: part.clone(),
            generation,
        })
    }

    /// The upload `key` was saved: its part is gone, later requests for it
    /// are answered with `finished`.
    pub(super) fn finish(&self, session: &str, key: &str, finished: Finished) {
        let mut sessions = self.lock();
        let entry = sessions.entry(session.to_string()).or_default();
        entry.parts.remove(key);
        if entry.finished.len() >= MAX_FINISHED {
            entry.finished.pop_front();
        }
        entry.finished.push_back((key.to_string(), finished));
    }

    /// Forgets an unfinished upload whose part file was deleted.
    pub(super) fn forget(&self, session: &str, key: &str) {
        if let Some(entry) = self.lock().get_mut(session) {
            entry.parts.remove(key);
        }
    }

    /// Deletes the unfinished uploads of a session (its window was closed).
    /// Blocking file system calls, but only a handful of deletions: cheap
    /// enough for the callers (window close, the once-per-second sweep).
    pub fn discard_session(&self, session: &str) {
        let Some(entry) = self.lock().remove(session) else {
            return;
        };
        for part in entry.parts.values() {
            let _ = std::fs::remove_file(&part.path);
        }
    }

    /// Deletes unfinished uploads whose session no longer exists.
    pub fn sweep(&self, manager: &ShareManager) {
        let gone: Vec<String> = self
            .lock()
            .keys()
            .filter(|id| manager.get(id).is_none())
            .cloned()
            .collect();
        for id in gone {
            self.discard_session(&id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_claim_takes_over() {
        let uploads = Uploads::default();
        let first = uploads.claim("s", "k", PathBuf::from("a")).unwrap();
        assert!(first.is_current());
        let second = uploads.claim("s", "k", PathBuf::from("b")).unwrap();
        assert!(!first.is_current());
        assert!(second.is_current());
        // Same part file for the same key.
        assert_eq!(second.part.path, PathBuf::from("a"));
        assert_eq!(uploads.part_path("s", "k"), Some(PathBuf::from("a")));
    }

    #[test]
    fn unfinished_uploads_are_limited_and_finished_ones_remembered() {
        let uploads = Uploads::default();
        for i in 0..MAX_PARTS {
            assert!(uploads
                .claim("s", &format!("k{i}"), PathBuf::new())
                .is_some());
        }
        assert!(uploads.claim("s", "one-more", PathBuf::new()).is_none());
        // Existing keys can still be resumed, other sessions are unaffected.
        assert!(uploads.claim("s", "k0", PathBuf::new()).is_some());
        assert!(uploads.claim("t", "k0", PathBuf::new()).is_some());

        let saved = Finished {
            name: "a.jpg".into(),
            size: 3,
        };
        uploads.finish("s", "k0", saved.clone());
        assert_eq!(uploads.finished("s", "k0"), Some(saved));
        assert_eq!(uploads.part_path("s", "k0"), None);
        assert!(uploads.claim("s", "one-more", PathBuf::new()).is_some());

        for i in 0..=MAX_FINISHED {
            let f = Finished {
                name: format!("{i}"),
                size: 0,
            };
            uploads.finish("t", &format!("f{i}"), f);
        }
        assert_eq!(uploads.finished("t", "f0"), None, "oldest is dropped");
        assert!(uploads.finished("t", &format!("f{MAX_FINISHED}")).is_some());

        uploads.discard_session("s");
        assert_eq!(uploads.finished("s", "k0"), None);
    }
}
