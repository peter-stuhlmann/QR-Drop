//! Progress bookkeeping shared by downloads and uploads: report throttling,
//! a smoothed transfer speed and the byte ranges a phone already received.

use std::ops::Range;
use std::time::{Duration, Instant};

/// Progress events at most 10× per second (CLAUDE.md §5.1).
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

/// Disjoint ranges tracked per share at most. A client asking for countless
/// scattered ranges only loses the "complete via ranges" detection, never
/// memory.
const MAX_RANGES: usize = 4096;

/// Decides when the next progress event is due and how fast the transfer is.
#[derive(Debug)]
pub(crate) struct RateMeter {
    /// Time and byte count of the last report (or of the start).
    last_at: Instant,
    last_value: u64,
    /// Smoothed bytes per second.
    rate: f64,
}

impl RateMeter {
    /// Starts measuring at `value` bytes, so bytes that were already there
    /// (a resumed transfer) never show up as a speed spike.
    pub fn new(value: u64) -> Self {
        Self {
            last_at: Instant::now(),
            last_value: value,
            rate: 0.0,
        }
    }

    /// Starts over, e.g. when a new transfer begins after a pause.
    pub fn restart(&mut self, value: u64) {
        *self = Self::new(value);
    }

    /// Returns the current speed if a report is due (`force` for the final
    /// one), otherwise `None`.
    pub fn sample(&mut self, value: u64, force: bool) -> Option<u64> {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_at);
        if !force && elapsed < PROGRESS_INTERVAL {
            return None;
        }
        let secs = elapsed.as_secs_f64().max(0.001);
        let instant_rate = value.saturating_sub(self.last_value) as f64 / secs;
        // Exponential smoothing so the MB/s display does not jitter.
        self.rate = if self.rate == 0.0 {
            instant_rate
        } else {
            self.rate * 0.7 + instant_rate * 0.3
        };
        self.last_at = now;
        self.last_value = value;
        Some(self.rate as u64)
    }
}

/// Byte ranges of a file that were handed to the phone, merged and sorted.
/// Lets clients that only ever use Range requests (video players, download
/// managers) complete a share.
#[derive(Debug, Default)]
pub(crate) struct Coverage {
    ranges: Vec<Range<u64>>,
}

impl Coverage {
    pub fn add(&mut self, range: Range<u64>) {
        if range.is_empty() {
            return;
        }
        // Fast path: sequential chunks extend the last range.
        if let Some(last) = self.ranges.last_mut() {
            if last.start <= range.start && range.start <= last.end {
                last.end = last.end.max(range.end);
                return;
            }
        }
        let mut merged = range;
        self.ranges.retain(|r| {
            let overlaps = r.start <= merged.end && merged.start <= r.end;
            if overlaps {
                merged.start = merged.start.min(r.start);
                merged.end = merged.end.max(r.end);
            }
            !overlaps
        });
        if self.ranges.len() < MAX_RANGES {
            let at = self.ranges.partition_point(|r| r.start < merged.start);
            self.ranges.insert(at, merged);
        }
    }

    /// Bytes covered in total.
    pub fn covered(&self) -> u64 {
        self.ranges.iter().map(|r| r.end - r.start).sum()
    }

    /// True once every byte of a file of `size` bytes was delivered.
    pub fn is_complete(&self, size: u64) -> bool {
        match self.ranges.as_slice() {
            [] => size == 0,
            [only] => only.start == 0 && only.end >= size,
            _ => false,
        }
    }

    pub fn clear(&mut self) {
        self.ranges.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_merges_ranges_in_any_order() {
        let mut c = Coverage::default();
        assert!(c.is_complete(0));
        assert!(!c.is_complete(10));
        c.add(5..8);
        c.add(0..2);
        c.add(8..8);
        assert_eq!(c.covered(), 5);
        assert!(!c.is_complete(10));
        c.add(1..6);
        c.add(8..10);
        assert_eq!(c.ranges.len(), 1, "merged into one range");
        assert!(c.is_complete(10));
        c.add(3..4);
        assert_eq!(c.covered(), 10);
        c.clear();
        assert_eq!(c.covered(), 0);
    }

    #[test]
    fn meter_starts_at_the_resume_point() {
        let mut m = RateMeter::new(1_000_000);
        assert_eq!(m.sample(1_000_000, false), None);
        // A forced report right away must not count the earlier bytes.
        let rate = m.sample(1_000_100, true).unwrap();
        assert!(rate <= 100_000, "{rate}");
    }
}
