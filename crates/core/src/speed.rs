//! Smoothed download speed and time left.

use std::time::Instant;

/// How quickly the smoothed speed follows a change, in seconds. After this long a
/// new speed has taken over about two thirds of the reading.
const SMOOTHING_SECS: f64 = 3.0;

/// An exponentially weighted moving average of bytes per second.
///
/// Samples may arrive at any interval; each one is weighted by the time it covers,
/// so an irregular tick does not distort the reading.
#[derive(Debug, Default, Clone)]
pub(crate) struct SpeedMeter {
    last: Option<(Instant, u64)>,
    rate: Option<f64>,
}

impl SpeedMeter {
    /// Records that `done` bytes had arrived by `at` and returns the smoothed speed.
    pub fn sample(&mut self, done: u64, at: Instant) -> u64 {
        if let Some((then, before)) = self.last {
            let secs = at.saturating_duration_since(then).as_secs_f64();
            if secs <= 0.0 {
                return self.speed();
            }
            let instant = done.saturating_sub(before) as f64 / secs;
            let weight = 1.0 - (-secs / SMOOTHING_SECS).exp();
            self.rate = Some(match self.rate {
                Some(rate) => rate + weight * (instant - rate),
                None => instant,
            });
        }
        self.last = Some((at, done));
        self.speed()
    }

    /// The current smoothed speed in bytes per second.
    pub fn speed(&self) -> u64 {
        self.rate.map_or(0, |r| r.round() as u64)
    }
}

/// Seconds until `total` is reached at `speed`, when both are known.
pub(crate) fn eta(total: Option<u64>, done: u64, speed: u64) -> Option<u64> {
    let total = total?;
    (speed > 0).then(|| total.saturating_sub(done).div_ceil(speed))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn steady_rate_reads_exactly() {
        let start = Instant::now();
        let mut meter = SpeedMeter::default();
        assert_eq!(meter.sample(0, start), 0);
        for i in 1..=20u64 {
            meter.sample(i * 100_000, start + Duration::from_millis(i * 100));
        }
        assert_eq!(meter.speed(), 1_000_000);
    }

    #[test]
    fn a_short_burst_is_smoothed() {
        let start = Instant::now();
        let mut meter = SpeedMeter::default();
        meter.sample(0, start);
        for i in 1..=30u64 {
            meter.sample(i * 100_000, start + Duration::from_millis(i * 100));
        }
        // One tick ten times faster than usual moves the reading only a little.
        let burst = meter.sample(4_000_000, start + Duration::from_millis(3100));
        assert!((1_000_000..1_400_000).contains(&burst), "{burst}");
    }

    #[test]
    fn a_stall_decays_towards_zero() {
        let start = Instant::now();
        let mut meter = SpeedMeter::default();
        meter.sample(0, start);
        meter.sample(1_000_000, start + Duration::from_secs(1));
        let after = meter.sample(1_000_000, start + Duration::from_secs(10));
        assert!(after < 60_000, "{after}");
    }

    #[test]
    fn eta_needs_size_and_speed() {
        assert_eq!(eta(Some(1000), 400, 100), Some(6));
        assert_eq!(eta(Some(1000), 950, 100), Some(1));
        assert_eq!(eta(None, 400, 100), None);
        assert_eq!(eta(Some(1000), 400, 0), None);
    }
}
