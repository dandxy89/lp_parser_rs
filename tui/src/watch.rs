//! Watch mode: poll input file mtimes on the tick event and reload when stable.
//!
//! Generators often write LP files in chunks, so a single mtime change is not a
//! safe reload trigger. [`WatchState`] is a pure debounce state machine: a change
//! is only reported once the new mtimes have been observed identically on two
//! consecutive polls. Parsing happens on a background thread (see
//! `App::poll_watch`) so the UI never blocks.

use std::path::Path;
use std::sync::mpsc;
use std::time::SystemTime;

use crate::parse::{ParsedFile, parse_file};

/// Result of a background reload: both files re-parsed, or a user-facing error.
pub type ReloadOutcome = Result<Box<(ParsedFile, ParsedFile)>, String>;

/// Watch-mode session state held on `App`.
pub struct WatchSession {
    /// Whether `--watch` was passed on the command line.
    pub enabled: bool,
    /// Debounce state machine fed from the tick-event mtime polls.
    pub state: WatchState,
    /// Channel for the in-flight background reload, if any. While this is
    /// `Some`, further triggers are ignored; polling re-arms once it completes.
    pub receive: Option<mpsc::Receiver<ReloadOutcome>>,
    /// Tick counter used to throttle mtime polls to every 5th tick (~250 ms),
    /// keeping `--watch` at ~8 `stat()` calls/sec instead of ~40. The debounce
    /// needs two stable observations, so reload latency stays under a second.
    pub ticks: u8,
}

impl WatchSession {
    /// A disabled session (the default when `--watch` is not given).
    pub const fn disabled() -> Self {
        Self { enabled: false, state: WatchState::new(None, None), receive: None, ticks: 0 }
    }

    /// Whether a background reload parse is currently in flight.
    pub const fn is_reloading(&self) -> bool {
        self.receive.is_some()
    }
}

/// A pair of observed mtimes, one per input file.
type MtimePair = (Option<SystemTime>, Option<SystemTime>);

/// Debounce state machine for file-change detection.
///
/// `observe` returns `true` (reload now) only when both mtimes are readable,
/// differ from the baseline recorded at the last trigger, and are identical to
/// the previous observation — i.e. the change has been stable for two
/// consecutive polls. Unreadable mtimes (file deleted mid-write) count as
/// "changed but not stable", so they never trigger a reload and never spam.
#[derive(Debug)]
pub struct WatchState {
    /// The mtimes as of the last reload trigger (or watch start).
    baseline: MtimePair,
    /// A change observed on the previous poll, awaiting confirmation.
    pending: Option<(SystemTime, SystemTime)>,
}

impl WatchState {
    /// Create a new state anchored at the given baseline mtimes.
    pub const fn new(mtime1: Option<SystemTime>, mtime2: Option<SystemTime>) -> Self {
        Self { baseline: (mtime1, mtime2), pending: None }
    }

    /// Feed one poll of both files' mtimes. Returns `true` when a reload
    /// should be attempted now; the new mtimes then become the baseline.
    pub fn observe(&mut self, mtime1: Option<SystemTime>, mtime2: Option<SystemTime>) -> bool {
        let (Some(first), Some(second)) = (mtime1, mtime2) else {
            // Unreadable mtime: treat as changed-but-not-stable. Drop any
            // pending change so a reload is only attempted once both files
            // are readable and stable again.
            self.pending = None;
            return false;
        };

        if (Some(first), Some(second)) == self.baseline {
            // Back to the baseline (e.g. a transient unreadable blip) — nothing to do.
            self.pending = None;
            return false;
        }

        match self.pending {
            Some(pending) if pending == (first, second) => {
                // Stable for two consecutive polls: trigger and re-anchor.
                self.baseline = (Some(first), Some(second));
                self.pending = None;
                true
            }
            _ => {
                // New or still-moving change: record and wait for confirmation.
                self.pending = Some((first, second));
                false
            }
        }
    }
}

/// Read a file's mtime, returning `None` on any error (e.g. file deleted).
pub fn read_mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|metadata| metadata.modified()).ok()
}

/// Re-parse both input files. Runs on a background thread; never panics the
/// caller — parse failures and vanished files surface as `Err(message)`.
pub fn reload_files(path1: &Path, path2: &Path) -> ReloadOutcome {
    // Guard against a file vanishing between the mtime poll and the parse:
    // `parse_file` debug-asserts existence, and a mid-write delete must surface
    // as a failed reload rather than a panic.
    for path in [path1, path2] {
        if !path.exists() {
            return Err(format!("file not found: '{}'", path.display()));
        }
    }

    let (result1, result2) = std::thread::scope(|scope| {
        let handle1 = scope.spawn(|| parse_file(path1));
        let handle2 = scope.spawn(|| parse_file(path2));
        (handle1.join(), handle2.join())
    });

    let unpack = |result: std::thread::Result<Result<ParsedFile, Box<dyn std::error::Error + Send + Sync>>>,
                  path: &Path|
     -> Result<ParsedFile, String> {
        match result {
            Ok(Ok(parsed)) => Ok(parsed),
            Ok(Err(error)) => Err(error.to_string()),
            Err(_) => Err(format!("parse thread for '{}' panicked", path.display())),
        }
    };

    let parsed1 = unpack(result1, path1)?;
    let parsed2 = unpack(result2, path2)?;
    Ok(Box::new((parsed1, parsed2)))
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use super::*;

    /// Helper: a deterministic mtime `n` seconds after the epoch.
    // Wrapped in Option so call sites match observe()'s parameters directly.
    #[allow(clippy::unnecessary_wraps)]
    fn mtime(n: u64) -> Option<SystemTime> {
        Some(UNIX_EPOCH + Duration::from_secs(n))
    }

    #[test]
    fn test_no_change_never_triggers() {
        let mut state = WatchState::new(mtime(10), mtime(20));
        for _ in 0..5 {
            assert!(!state.observe(mtime(10), mtime(20)));
        }
    }

    #[test]
    fn test_change_triggers_only_after_two_stable_polls() {
        let mut state = WatchState::new(mtime(10), mtime(20));
        // First poll seeing the change: pending, no trigger.
        assert!(!state.observe(mtime(11), mtime(20)));
        // Second identical poll: stable → trigger.
        assert!(state.observe(mtime(11), mtime(20)));
        // Third poll with the same mtimes: now the baseline, no re-trigger.
        assert!(!state.observe(mtime(11), mtime(20)));
    }

    #[test]
    fn test_still_moving_mtime_keeps_deferring() {
        let mut state = WatchState::new(mtime(10), mtime(20));
        // Generator writing in chunks: mtime keeps advancing every poll.
        assert!(!state.observe(mtime(11), mtime(20)));
        assert!(!state.observe(mtime(12), mtime(20)));
        assert!(!state.observe(mtime(13), mtime(20)));
        // Finally settles.
        assert!(state.observe(mtime(13), mtime(20)));
    }

    #[test]
    fn test_unreadable_mtime_never_triggers() {
        let mut state = WatchState::new(mtime(10), mtime(20));
        assert!(!state.observe(None, mtime(20)));
        assert!(!state.observe(None, mtime(20)));
        assert!(!state.observe(mtime(10), None));
    }

    #[test]
    fn test_unreadable_blip_clears_pending() {
        let mut state = WatchState::new(mtime(10), mtime(20));
        assert!(!state.observe(mtime(11), mtime(20))); // pending
        assert!(!state.observe(None, mtime(20))); // file briefly unreadable — pending dropped
        // Stability must be re-established from scratch.
        assert!(!state.observe(mtime(11), mtime(20)));
        assert!(state.observe(mtime(11), mtime(20)));
    }

    #[test]
    fn test_revert_to_baseline_cancels_pending() {
        let mut state = WatchState::new(mtime(10), mtime(20));
        assert!(!state.observe(mtime(11), mtime(20))); // pending
        assert!(!state.observe(mtime(10), mtime(20))); // back to baseline — cancelled
        assert!(!state.observe(mtime(10), mtime(20)));
    }

    #[test]
    fn test_second_change_after_trigger_retriggers() {
        let mut state = WatchState::new(mtime(10), mtime(20));
        assert!(!state.observe(mtime(11), mtime(20)));
        assert!(state.observe(mtime(11), mtime(20)));
        // A later change to the other file triggers again after stabilising.
        assert!(!state.observe(mtime(11), mtime(21)));
        assert!(state.observe(mtime(11), mtime(21)));
    }

    #[test]
    fn test_baseline_unreadable_then_readable_triggers_after_stability() {
        // Watch started while a file was missing: baseline has None.
        let mut state = WatchState::new(None, mtime(20));
        assert!(!state.observe(mtime(5), mtime(20)));
        assert!(state.observe(mtime(5), mtime(20)));
    }
}
