//! Suspend/resume detection by comparing wall-clock and monotonic elapsed time.

use std::time::{Duration, Instant, SystemTime};

/// Default wall-vs-monotonic gap (seconds) treated as a suspend/resume.
pub const DEFAULT_RESUME_THRESHOLD_SECS: u64 = 10;

/// Returns `true` when wall time advanced more than monotonic time plus `threshold`.
pub fn detect_resume_gap(
    wall_elapsed: Duration,
    mono_elapsed: Duration,
    threshold: Duration,
) -> bool {
    wall_elapsed > mono_elapsed.saturating_add(threshold)
}

/// Detects system resume between successive [`ResumeDetector::check`] calls.
#[derive(Debug, Clone)]
pub struct ResumeDetector {
    last_wall: SystemTime,
    last_mono: Instant,
    threshold: Duration,
}

impl ResumeDetector {
    /// Creates a detector with the given gap `threshold`.
    pub fn new(threshold: Duration) -> Self {
        Self {
            last_wall: SystemTime::now(),
            last_mono: Instant::now(),
            threshold,
        }
    }

    /// Samples clocks, updates state, and returns `true` if a resume gap was detected.
    pub fn check(&mut self) -> bool {
        let now_wall = SystemTime::now();
        let now_mono = Instant::now();
        let wall_elapsed = now_wall
            .duration_since(self.last_wall)
            .unwrap_or(Duration::ZERO);
        let mono_elapsed = now_mono.saturating_duration_since(self.last_mono);
        self.last_wall = now_wall;
        self.last_mono = now_mono;
        detect_resume_gap(wall_elapsed, mono_elapsed, self.threshold)
    }
}

impl Default for ResumeDetector {
    fn default() -> Self {
        Self::new(Duration::from_secs(DEFAULT_RESUME_THRESHOLD_SECS))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: Duration = Duration::from_secs(10);

    #[test]
    fn normal_tick_is_not_resume() {
        assert!(!detect_resume_gap(
            Duration::from_secs(2),
            Duration::from_secs(2),
            T
        ));
    }

    #[test]
    fn small_ntp_drift_is_not_resume() {
        assert!(!detect_resume_gap(
            Duration::from_secs(7),
            Duration::from_secs(2),
            T
        ));
    }

    #[test]
    fn clock_backward_is_not_resume() {
        assert!(!detect_resume_gap(
            Duration::ZERO,
            Duration::from_secs(2),
            T
        ));
    }

    #[test]
    fn suspend_gap_is_resume() {
        assert!(detect_resume_gap(
            Duration::from_secs(600),
            Duration::from_secs(2),
            T
        ));
    }

    #[test]
    fn detector_no_gap_immediately() {
        let mut d = ResumeDetector::default();
        assert!(!d.check());
    }
}
