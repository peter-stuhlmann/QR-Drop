//! Registry of running shares: tokens, lifetime, single-download semantics
//! and the transfers currently in flight.

use std::collections::HashMap;
use std::fmt;
use std::ops::Range;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use super::progress::{Coverage, RateMeter};
use super::{new_token, token_eq, ShareSource};

/// Default lifetime of a share (CLAUDE.md §5.2). For receive sessions it is
/// the idle timeout: every transferred chunk starts it over.
pub const DEFAULT_TTL: Duration = Duration::from_secs(10 * 60);

/// Passwords and other secrets are only valid briefly.
pub const SECRET_TTL: Duration = Duration::from_secs(2 * 60);

/// Hard limit for a share, however busy it is: a receive session that keeps
/// getting data still ends an hour after it was created or renewed.
pub const DEFAULT_MAX_LIFETIME: Duration = Duration::from_secs(60 * 60);

#[derive(Debug, Clone)]
pub struct ShareConfig {
    pub ttl: Duration,
    /// End the share after the first complete download.
    pub single_download: bool,
    /// Absolute limit from creation / renewal, see [`DEFAULT_MAX_LIFETIME`].
    pub max_lifetime: Duration,
}

impl Default for ShareConfig {
    fn default() -> Self {
        Self {
            ttl: DEFAULT_TTL,
            single_download: true,
            max_lifetime: DEFAULT_MAX_LIFETIME,
        }
    }
}

#[derive(Clone)]
pub struct ActiveShare {
    pub id: String,
    pub token: String,
    pub source: Arc<ShareSource>,
    pub expires_at: Instant,
    /// Lifetime of this share (shorter for secrets).
    pub ttl: Duration,
    /// Cached preview JPEG (None = not computed yet).
    pub preview: Arc<tokio::sync::OnceCell<Option<bytes::Bytes>>>,
}

/// The token is the share's only secret, so it never appears in Debug output.
impl fmt::Debug for ActiveShare {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ActiveShare")
            .field("id", &self.id)
            .field("token", &"<redacted>")
            .field("source", &self.source)
            .field("expires_at", &self.expires_at)
            .field("ttl", &self.ttl)
            .finish_non_exhaustive()
    }
}

/// What a token currently stands for.
#[derive(Debug)]
pub(crate) enum Lookup {
    Valid(ActiveShare),
    /// The single download / text was delivered at `at`.
    Done {
        share: ActiveShare,
        at: Instant,
    },
    Expired,
    /// Never existed, cancelled or replaced by "Neuen Code erstellen".
    Unknown,
}

/// How a running transfer reacts when its share ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransferKind {
    /// A complete download (whole file or ZIP). Stops as soon as another
    /// download completed the single-download share.
    Full,
    /// A Range request. May finish after the share completed (the phone
    /// re-requests the last bytes).
    Partial,
    /// A file from the phone.
    Upload,
}

#[derive(Debug)]
enum State {
    Open,
    Done { at: Instant },
    Expired,
}

#[derive(Debug)]
struct Entry {
    share: ActiveShare,
    connected: bool,
    state: State,
    /// Start of the current lifetime (creation or renewal).
    started_at: Instant,
    /// Downloads and uploads in flight. A share never expires under a
    /// running transfer; it ends once the transfer is over.
    active_transfers: u32,
    /// File bytes delivered so far, for clients that only use ranges.
    delivered: Coverage,
    /// Download progress of the share, at most 10 reports per second even
    /// with several parallel requests.
    meter: RateMeter,
}

impl Entry {
    /// Expired by time but not marked yet.
    fn lapsed(&self, now: Instant) -> bool {
        // Links live in the QR code itself and do not expire.
        let is_link = matches!(*self.share.source, ShareSource::Url { .. });
        matches!(self.state, State::Open)
            && !is_link
            && self.active_transfers == 0
            && self.share.expires_at <= now
    }

    /// Fresh token and lifetime.
    fn restart(&mut self, token: String, max_lifetime: Duration) {
        let now = Instant::now();
        self.share.token = token;
        self.share.expires_at = now + self.share.ttl.min(max_lifetime);
        self.started_at = now;
        self.connected = false;
        self.state = State::Open;
        self.delivered.clear();
    }
}

/// Thread-safe registry of all running shares.
pub struct ShareManager {
    inner: Mutex<HashMap<String, Entry>>,
    config: ShareConfig,
}

impl ShareManager {
    pub fn new(config: ShareConfig) -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            config,
        }
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<String, Entry>> {
        // A poisoned lock only means another thread panicked mid-update; the
        // map itself is still usable.
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn config(&self) -> &ShareConfig {
        &self.config
    }

    /// Registers a share under an id chosen by the caller (the window is
    /// opened before the files are scanned).
    pub fn create_with_id(
        &self,
        id: String,
        source: ShareSource,
    ) -> Result<ActiveShare, getrandom::Error> {
        let ttl = if source.is_secret() {
            SECRET_TTL.min(self.config.ttl)
        } else {
            self.config.ttl
        };
        let now = Instant::now();
        let share = ActiveShare {
            id,
            token: new_token()?,
            source: Arc::new(source),
            expires_at: now + ttl.min(self.config.max_lifetime),
            ttl,
            preview: Arc::new(tokio::sync::OnceCell::new()),
        };
        self.lock().insert(
            share.id.clone(),
            Entry {
                share: share.clone(),
                connected: false,
                state: State::Open,
                started_at: now,
                active_transfers: 0,
                delivered: Coverage::default(),
                meter: RateMeter::new(0),
            },
        );
        Ok(share)
    }

    /// What `token` stands for right now.
    pub(crate) fn lookup(&self, token: &str) -> Lookup {
        let now = Instant::now();
        let inner = self.lock();
        let Some(entry) = inner.values().find(|e| token_eq(&e.share.token, token)) else {
            return Lookup::Unknown;
        };
        match entry.state {
            State::Open if entry.lapsed(now) => Lookup::Expired,
            State::Open => Lookup::Valid(entry.share.clone()),
            State::Done { at } => Lookup::Done {
                share: entry.share.clone(),
                at,
            },
            State::Expired => Lookup::Expired,
        }
    }

    /// Valid (existing, not expired, not finished) share for a token.
    pub fn by_token(&self, token: &str) -> Option<ActiveShare> {
        match self.lookup(token) {
            Lookup::Valid(share) => Some(share),
            _ => None,
        }
    }

    /// Whether the share's code has run out -- never while a transfer is
    /// still running (the window must not show "abgelaufen" mid-download).
    pub fn is_expired(&self, id: &str) -> bool {
        let now = Instant::now();
        self.lock().get(id).is_some_and(|e| match e.state {
            State::Open => e.lapsed(now),
            State::Expired => true,
            State::Done { .. } => false,
        })
    }

    pub fn get(&self, id: &str) -> Option<ActiveShare> {
        self.lock().get(id).map(|e| e.share.clone())
    }

    /// Returns true the first time the phone connects.
    pub fn mark_connected(&self, id: &str) -> bool {
        match self.lock().get_mut(id) {
            Some(e) if !e.connected => {
                e.connected = true;
                true
            }
            _ => false,
        }
    }

    /// Records a complete delivery (download or text). Returns true if it
    /// counts and should be reported as done: in single-download mode only
    /// for the first one, which closes the share; later ones lost the race.
    /// With several downloads allowed every delivery counts.
    pub(crate) fn record_download(&self, id: &str) -> bool {
        let mut inner = self.lock();
        let Some(e) = inner.get_mut(id) else {
            return false;
        };
        if !matches!(e.state, State::Open) {
            return false;
        }
        if self.config.single_download {
            e.state = State::Done { at: Instant::now() };
        } else {
            // The next download is tracked from scratch.
            e.delivered.clear();
        }
        true
    }

    /// New token and fresh lifetime for the same files ("Neuen Code erstellen").
    pub fn renew(&self, id: &str) -> Option<ActiveShare> {
        let token = new_token().ok()?;
        let mut inner = self.lock();
        let entry = inner.get_mut(id)?;
        entry.restart(token, self.config.max_lifetime);
        Some(entry.share.clone())
    }

    /// Real data arrived for a receive session: the idle timeout starts
    /// over, but never beyond the absolute limit. Opening the page or asking
    /// for free space does not count, so an idle session still ends.
    pub fn touch(&self, id: &str) {
        let now = Instant::now();
        if let Some(e) = self.lock().get_mut(id) {
            if matches!(e.state, State::Open) {
                let limit = e.started_at + self.config.max_lifetime;
                e.share.expires_at = (now + e.share.ttl).min(limit);
            }
        }
    }

    /// Ends a share for good (window closed). Running transfers notice it at
    /// their next chunk and stop.
    pub fn remove(&self, id: &str) -> Option<ActiveShare> {
        self.lock().remove(id).map(|e| e.share)
    }

    /// Ids of shares that just expired. They stay registered (so the window
    /// can offer "Neuen Code erstellen") but their token no longer works.
    /// A share with a running transfer expires only after it.
    pub fn take_expired(&self, now: Instant) -> Vec<String> {
        let mut inner = self.lock();
        let mut ids = Vec::new();
        for e in inner.values_mut() {
            if e.lapsed(now) {
                e.state = State::Expired;
                ids.push(e.share.id.clone());
            }
        }
        ids
    }

    pub fn len(&self) -> usize {
        self.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Registers a running transfer; it keeps the share from expiring until
    /// the returned guard is finished or dropped.
    pub(crate) fn begin_transfer(self: &Arc<Self>, id: &str, kind: TransferKind) -> Transfer {
        if let Some(e) = self.lock().get_mut(id) {
            if e.active_transfers == 0 {
                // Measure the speed from now on, not since the last pause.
                e.meter.restart(e.delivered.covered());
            }
            e.active_transfers += 1;
        }
        Transfer {
            manager: self.clone(),
            id: id.to_string(),
            kind,
            ended: false,
        }
    }

    fn transfer_cancelled(&self, id: &str, kind: TransferKind) -> bool {
        match self.lock().get(id) {
            None => true,
            Some(e) => kind == TransferKind::Full && matches!(e.state, State::Done { .. }),
        }
    }

    /// Returns true if an interruption should be reported: the transfer
    /// broke off, no other one is running and the share is still open.
    fn end_transfer(&self, id: &str, broke_off: bool) -> bool {
        let mut inner = self.lock();
        let Some(e) = inner.get_mut(id) else {
            return false;
        };
        e.active_transfers = e.active_transfers.saturating_sub(1);
        broke_off && e.active_transfers == 0 && matches!(e.state, State::Open)
    }

    /// Notes that `range` of the shared file reached the phone. Returns the
    /// bytes delivered so far and whether this completed the file.
    pub(crate) fn add_delivered(&self, id: &str, range: Range<u64>, size: u64) -> (u64, bool) {
        let mut inner = self.lock();
        let Some(e) = inner.get_mut(id) else {
            return (0, false);
        };
        let was_complete = e.delivered.is_complete(size);
        e.delivered.add(range);
        (
            e.delivered.covered(),
            !was_complete && e.delivered.is_complete(size),
        )
    }

    /// Speed for a progress report of the share if one is due (see
    /// [`RateMeter::sample`]).
    pub(crate) fn progress_sample(&self, id: &str, value: u64, force: bool) -> Option<u64> {
        self.lock().get_mut(id)?.meter.sample(value, force)
    }
}

/// A download or upload in flight. Keeps its share from expiring and tells
/// the transfer when to stop.
pub(crate) struct Transfer {
    manager: Arc<ShareManager>,
    id: String,
    kind: TransferKind,
    ended: bool,
}

impl Transfer {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn manager(&self) -> &ShareManager {
        &self.manager
    }

    /// True once the share was closed on the computer, or (for a complete
    /// download) another download already delivered the single copy.
    pub fn is_cancelled(&self) -> bool {
        self.manager.transfer_cancelled(&self.id, self.kind)
    }

    /// The transfer completed.
    /// This transfer delivered the single copy (e.g. it completed bytes an
    /// earlier Range request already sent). It may send its remaining bytes;
    /// from now on only closing the share stops it.
    pub fn delivered_share(&mut self) {
        self.kind = TransferKind::Partial;
    }

    pub fn finish(&mut self) {
        if !std::mem::replace(&mut self.ended, true) {
            self.manager.end_transfer(&self.id, false);
        }
    }

    /// The transfer broke off. Returns true if that should be reported as
    /// an interruption (see [`ShareManager::end_transfer`]).
    pub fn abort(&mut self) -> bool {
        !std::mem::replace(&mut self.ended, true) && self.manager.end_transfer(&self.id, true)
    }
}

impl Drop for Transfer {
    fn drop(&mut self) {
        self.finish();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::share::new_id;
    use std::path::PathBuf;

    fn file_share() -> ShareSource {
        ShareSource::File {
            path: PathBuf::from("a.txt"),
            name: "a.txt".into(),
            size: 3,
        }
    }

    #[test]
    fn is_expired_waits_for_running_transfers() {
        let m = Arc::new(ShareManager::new(ShareConfig {
            ttl: Duration::from_millis(0),
            ..ShareConfig::default()
        }));
        let s = m.create_with_id(new_id().unwrap(), file_share()).unwrap();
        let transfer = m.begin_transfer(&s.id, TransferKind::Full);
        assert!(
            !m.is_expired(&s.id),
            "never expired under a running transfer"
        );
        drop(transfer);
        assert!(m.is_expired(&s.id));
        assert!(!m.is_expired("unknown"));
    }

    fn create(m: &ShareManager, source: ShareSource) -> ActiveShare {
        m.create_with_id(new_id().unwrap(), source).unwrap()
    }

    fn config(ttl: Duration) -> ShareConfig {
        ShareConfig {
            ttl,
            ..ShareConfig::default()
        }
    }

    #[test]
    fn token_lookup_requires_exact_match() {
        let m = ShareManager::new(ShareConfig::default());
        let s = create(&m, file_share());
        assert!(m.by_token(&s.token).is_some());
        assert!(m.by_token(&s.token[..21]).is_none());
        assert!(m.by_token("").is_none());
        let mut wrong = s.token.clone();
        wrong.replace_range(0..1, if wrong.starts_with('A') { "B" } else { "A" });
        assert!(m.by_token(&wrong).is_none());
        assert!(matches!(m.lookup(&wrong), Lookup::Unknown));
    }

    #[test]
    fn share_expires_and_renews() {
        let m = ShareManager::new(config(Duration::ZERO));
        let s = create(&m, file_share());
        assert!(m.by_token(&s.token).is_none());
        assert!(matches!(m.lookup(&s.token), Lookup::Expired));
        assert_eq!(m.take_expired(Instant::now()), vec![s.id.clone()]);
        assert!(m.take_expired(Instant::now()).is_empty());
        assert!(matches!(m.lookup(&s.token), Lookup::Expired));
        // Renewing gives a new token; the old one is unknown from then on.
        let renewed = m.renew(&s.id).unwrap();
        assert_ne!(renewed.token, s.token);
        assert!(matches!(m.lookup(&s.token), Lookup::Unknown));
    }

    #[test]
    fn single_download_closes_share() {
        let m = ShareManager::new(ShareConfig::default());
        let s = create(&m, file_share());
        assert!(m.record_download(&s.id));
        assert!(!m.record_download(&s.id));
        assert!(m.by_token(&s.token).is_none());
        assert!(matches!(m.lookup(&s.token), Lookup::Done { .. }));
    }

    #[test]
    fn multi_download_keeps_share() {
        let m = ShareManager::new(ShareConfig {
            single_download: false,
            ..ShareConfig::default()
        });
        let s = create(&m, file_share());
        assert!(m.record_download(&s.id));
        assert!(m.record_download(&s.id));
        assert!(m.by_token(&s.token).is_some());
    }

    #[test]
    fn secrets_expire_sooner_and_links_never() {
        let m = ShareManager::new(ShareConfig::default());
        let secret = create(&m, ShareSource::from_text("Tr0ub4dor&3").unwrap());
        assert_eq!(secret.ttl, SECRET_TTL);
        let text = create(&m, ShareSource::from_text("Hallo Welt").unwrap());
        assert_eq!(text.ttl, DEFAULT_TTL);
        let link = create(&m, ShareSource::from_text("https://example.com").unwrap());
        let later = Instant::now() + Duration::from_secs(3600);
        let expired = m.take_expired(later);
        assert!(expired.contains(&secret.id));
        assert!(expired.contains(&text.id));
        assert!(!expired.contains(&link.id));
    }

    #[test]
    fn touch_extends_receive_sessions_up_to_the_limit() {
        let m = ShareManager::new(ShareConfig {
            ttl: Duration::from_millis(200),
            max_lifetime: Duration::from_millis(400),
            ..ShareConfig::default()
        });
        let s = create(
            &m,
            ShareSource::Receive {
                dir: PathBuf::from("Downloads"),
            },
        );
        assert_eq!(s.source.name(), "Downloads");
        std::thread::sleep(Duration::from_millis(120));
        m.touch(&s.id);
        std::thread::sleep(Duration::from_millis(120));
        assert!(m.by_token(&s.token).is_some());
        assert!(m.take_expired(Instant::now()).is_empty());
        // Busy or not: the absolute limit ends the session.
        for _ in 0..4 {
            std::thread::sleep(Duration::from_millis(60));
            m.touch(&s.id);
        }
        assert!(m.by_token(&s.token).is_none());
        // Renewing starts a fresh lifetime.
        let renewed = m.renew(&s.id).unwrap();
        assert!(m.by_token(&renewed.token).is_some());
    }

    #[test]
    fn running_transfers_delay_expiry_and_notice_cancellation() {
        let m = Arc::new(ShareManager::new(config(Duration::ZERO)));
        let s = create(&m, file_share());
        let mut t = m.begin_transfer(&s.id, TransferKind::Full);
        assert!(m.take_expired(Instant::now()).is_empty());
        assert!(!t.is_cancelled());
        // Breaking off as the only transfer of an open share is reported.
        assert!(t.abort());
        assert_eq!(m.take_expired(Instant::now()), vec![s.id.clone()]);

        let m = Arc::new(ShareManager::new(ShareConfig::default()));
        let s = create(&m, file_share());
        let full = m.begin_transfer(&s.id, TransferKind::Full);
        let mut other = m.begin_transfer(&s.id, TransferKind::Full);
        let partial = m.begin_transfer(&s.id, TransferKind::Partial);
        assert!(m.record_download(&s.id));
        // The single copy is out: other complete downloads stop, ranges may finish.
        assert!(full.is_cancelled());
        assert!(!partial.is_cancelled());
        assert!(!other.abort(), "no interruption once the share is done");
        m.remove(&s.id);
        assert!(partial.is_cancelled());
    }

    #[test]
    fn ranges_complete_the_file() {
        let m = ShareManager::new(ShareConfig::default());
        let s = create(&m, file_share());
        assert_eq!(m.add_delivered(&s.id, 0..2, 3), (2, false));
        assert_eq!(m.add_delivered(&s.id, 2..3, 3), (3, true));
        assert_eq!(m.add_delivered(&s.id, 1..3, 3), (3, false));
    }

    #[test]
    fn debug_hides_the_token() {
        let m = ShareManager::new(ShareConfig::default());
        let s = create(&m, file_share());
        assert!(!format!("{s:?}").contains(&s.token));
    }
}
