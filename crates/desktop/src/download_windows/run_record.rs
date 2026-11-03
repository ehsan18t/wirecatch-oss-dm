//! What a download window saw of a download's run: its speed at each point of the file, how long
//! it really ran, and its average and peak. The app keeps no such history, so a window knows only
//! what it watched.

use std::time::{Duration, Instant};

use wirecatch_protocol::app::PER_MILLE_DONE;

const SAMPLE_EVERY: Duration = Duration::from_millis(500);
/// Most samples kept: every other one goes when full, so a long run keeps its whole shape.
const MOST_SAMPLES: usize = 240;
/// Fewest samples that make a graph; a run shorter than this has no shape to show.
const FEWEST_DRAWN: usize = 3;
/// Shorter than this, an average is mostly noise.
const SHORTEST_AVERAGE: Duration = Duration::from_secs(1);
/// The engine retries on its own and says nothing: this long with no bytes is all that shows it.
const STALLED_AFTER: Duration = Duration::from_secs(3);

/// One look at the running download.
#[derive(Clone, Copy, Debug)]
pub struct Look {
    /// How far it is, in tenths of a percent, when its size is known.
    pub per_mille: Option<u16>,
    pub speed: u64,
    pub done: u64,
    /// It is moving data now; time spent paused or waiting does not count.
    pub running: bool,
}

#[derive(Clone, Copy, Debug)]
struct Sample {
    per_mille: Option<u16>,
    speed: u64,
}

#[derive(Default)]
pub struct RunRecord {
    samples: Vec<Sample>,
    ran: Duration,
    last_seen_running: Option<Instant>,
    last_moved: Option<Instant>,
    last_sampled: Option<Instant>,
    sampled_done: Option<u64>,
    first_done: Option<u64>,
    last_done: u64,
    peak: u64,
}

impl RunRecord {
    pub fn see(&mut self, look: Look, now: Instant) {
        if !look.running {
            self.last_seen_running = None;
            self.last_moved = None;
            return;
        }
        // A server that cannot resume starts the file over: so do the figures for this run.
        if look.done < self.last_done {
            self.ran = Duration::ZERO;
            self.first_done = None;
            self.last_seen_running = None;
            self.sampled_done = None;
            self.samples.clear();
            self.peak = 0;
        }
        let waiting = look.done == self.last_done && self.stalled(now);
        // A gap this long means no frames were drawn during a stall: it did not run either.
        if let Some(last) = self.last_seen_running
            && !waiting
            && now.saturating_duration_since(last) < STALLED_AFTER
        {
            self.ran += now.saturating_duration_since(last);
        }
        self.last_seen_running = Some(now);
        self.first_done.get_or_insert(look.done);
        if self.last_moved.is_none() || look.done != self.last_done {
            self.last_moved = Some(now);
        }
        self.last_done = look.done;
        self.peak = self.peak.max(look.speed);
        let due = self
            .last_sampled
            .is_none_or(|last| now.saturating_duration_since(last) >= SAMPLE_EVERY);
        // The reported speed decays while no bytes arrive: drawn, a stalled end looks like a cliff.
        let moved = self.sampled_done.is_none_or(|was| look.done > was);
        if !due || !moved || look.speed == 0 {
            return;
        }
        self.last_sampled = Some(now);
        self.sampled_done = Some(look.done);
        if self.samples.len() == MOST_SAMPLES {
            let mut keep = false;
            self.samples.retain(|_| {
                keep = !keep;
                keep
            });
        }
        self.samples.push(Sample {
            per_mille: look.per_mille,
            speed: look.speed,
        });
    }

    /// Whether there is enough for a graph.
    pub fn drawable(&self) -> bool {
        self.samples.len() >= FEWEST_DRAWN
    }

    /// Each sample as how far across (0 to 1) and its speed: across the file when its size is
    /// known, else across the time watched. Runs from the left edge to `end`, the nearest sample's
    /// speed filling what the window did not watch.
    pub fn points(&self, end: f32) -> Vec<(f32, u64)> {
        let last = self.samples.len().saturating_sub(1).max(1);
        let mut points: Vec<(f32, u64)> = self
            .samples
            .iter()
            .enumerate()
            .map(|(index, sample)| {
                let across = sample.per_mille.map_or_else(
                    || {
                        let part = u16::try_from(index * 1000 / last).unwrap_or(1000);
                        f32::from(part) / 1000.
                    },
                    |at| f32::from(at) / f32::from(PER_MILLE_DONE),
                );
                (across, sample.speed)
            })
            .collect();
        if let Some(&(across, speed)) = points.first()
            && across > 0.
        {
            points.insert(0, (0., speed));
        }
        if let Some(&(across, speed)) = points.last()
            && across < end
        {
            points.push((end, speed));
        }
        points
    }

    /// Whether it should be moving but no bytes have come for a while.
    pub fn stalled(&self, now: Instant) -> bool {
        self.last_moved
            .is_some_and(|moved| now.saturating_duration_since(moved) >= STALLED_AFTER)
    }

    pub fn ran(&self) -> Option<Duration> {
        (!self.ran.is_zero()).then_some(self.ran)
    }

    pub fn peak(&self) -> Option<u64> {
        (self.peak > 0).then_some(self.peak)
    }

    /// Bytes a second over the time it ran, once it ran long enough to say.
    pub fn average(&self) -> Option<u64> {
        if self.ran < SHORTEST_AVERAGE {
            return None;
        }
        let gained = self.last_done
            - self
                .first_done
                .unwrap_or(self.last_done)
                .min(self.last_done);
        let millis = u64::try_from(self.ran.as_millis()).ok()?.max(1);
        Some(gained.saturating_mul(1000) / millis)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn running(per_mille: u16, speed: u64, done: u64) -> Look {
        Look {
            per_mille: Some(per_mille),
            speed,
            done,
            running: true,
        }
    }

    #[test]
    fn time_paused_does_not_count_as_running() {
        let start = Instant::now();
        let mut record = RunRecord::default();
        record.see(running(100, 10, 0), start);
        record.see(running(200, 10, 2000), start + Duration::from_secs(2));
        record.see(
            Look {
                running: false,
                ..running(200, 0, 2000)
            },
            start + Duration::from_secs(3),
        );
        record.see(running(200, 10, 2000), start + Duration::from_secs(60));
        record.see(running(400, 10, 4000), start + Duration::from_secs(62));
        assert_eq!(record.ran(), Some(Duration::from_secs(4)));
        assert_eq!(record.average(), Some(1000));
    }

    #[test]
    fn looks_within_the_sampling_gap_keep_one_sample() {
        let start = Instant::now();
        let mut record = RunRecord::default();
        record.see(running(100, 10, 1), start);
        record.see(running(110, 90, 2), start + Duration::from_millis(100));
        record.see(running(120, 20, 3), start + SAMPLE_EVERY);
        assert_eq!(record.points(0.12), [(0., 10), (0.1, 10), (0.12, 20)]);
        assert_eq!(record.peak(), Some(90), "the peak sees every look");
    }

    #[test]
    fn a_file_started_over_starts_the_graph_over() {
        let start = Instant::now();
        let mut record = RunRecord::default();
        record.see(running(500, 10, 5000), start);
        record.see(running(600, 10, 6000), start + SAMPLE_EVERY);
        record.see(running(50, 10, 500), start + SAMPLE_EVERY * 2);
        assert_eq!(record.points(0.05), [(0., 10), (0.05, 10)]);
    }

    #[test]
    fn a_file_started_over_starts_the_average_over() {
        let start = Instant::now();
        let mut record = RunRecord::default();
        record.see(running(100, 10, 0), start);
        record.see(running(500, 10, 5000), start + Duration::from_secs(10));
        record.see(running(10, 10, 100), start + Duration::from_secs(11));
        record.see(running(110, 10, 2100), start + Duration::from_secs(13));
        assert_eq!(record.ran(), Some(Duration::from_secs(2)));
        assert_eq!(record.average(), Some(1000));
    }

    #[test]
    fn a_long_run_thins_its_samples_and_keeps_its_ends() {
        let start = Instant::now();
        let mut record = RunRecord::default();
        for step in 0..=u32::try_from(MOST_SAMPLES).unwrap() {
            let at = u16::try_from(step).unwrap();
            record.see(
                running(at, 10, u64::from(step)),
                start + SAMPLE_EVERY * step,
            );
        }
        let points = record.points(f32::from(u16::try_from(MOST_SAMPLES).unwrap()) / 1000.);
        assert!(points.len() <= MOST_SAMPLES);
        assert_eq!(points.first().map(|point| point.0), Some(0.));
        assert_eq!(
            points.last().map(|point| point.0),
            Some(f32::from(u16::try_from(MOST_SAMPLES).unwrap()) / 1000.)
        );
    }

    #[test]
    fn an_unknown_size_spreads_across_the_time_watched() {
        let start = Instant::now();
        let mut record = RunRecord::default();
        for step in 0..3u32 {
            record.see(
                Look {
                    per_mille: None,
                    ..running(0, 10, u64::from(step))
                },
                start + SAMPLE_EVERY * step,
            );
        }
        assert!(record.drawable());
        assert_eq!(record.points(1.), [(0., 10), (0.5, 10), (1., 10)]);
    }

    #[test]
    fn the_curve_runs_from_the_start_to_the_end_asked_for() {
        let start = Instant::now();
        let mut record = RunRecord::default();
        record.see(running(100, 10, 1), start);
        record.see(running(500, 30, 2), start + SAMPLE_EVERY);
        assert_eq!(
            record.points(0.8),
            [(0., 10), (0.1, 10), (0.5, 30), (0.8, 30)]
        );
        assert_eq!(record.points(1.).last(), Some(&(1., 30)));
    }

    #[test]
    fn a_speed_falling_with_no_new_bytes_is_not_drawn() {
        let start = Instant::now();
        let mut record = RunRecord::default();
        record.see(running(100, 30, 1000), start);
        record.see(running(500, 30, 5000), start + SAMPLE_EVERY);
        record.see(running(500, 2, 5000), start + SAMPLE_EVERY * 2);
        assert_eq!(record.points(0.5), [(0., 30), (0.1, 30), (0.5, 30)]);
    }

    #[test]
    fn no_new_bytes_for_a_while_is_a_stall_until_bytes_come() {
        let start = Instant::now();
        let mut record = RunRecord::default();
        record.see(running(100, 10, 1000), start);
        record.see(running(100, 5, 1000), start + STALLED_AFTER);
        assert!(record.stalled(start + STALLED_AFTER));
        let later = start + STALLED_AFTER * 2;
        record.see(running(200, 5, 2000), later);
        assert!(!record.stalled(later));
    }

    #[test]
    fn time_stalled_does_not_count_as_running() {
        let start = Instant::now();
        let mut record = RunRecord::default();
        record.see(running(100, 10, 0), start);
        record.see(running(200, 10, 2000), start + Duration::from_secs(2));
        record.see(running(200, 1, 2000), start + Duration::from_secs(60));
        record.see(running(300, 10, 4000), start + Duration::from_secs(62));
        assert_eq!(record.ran(), Some(Duration::from_secs(4)));
        assert_eq!(record.average(), Some(1000));
    }

    #[test]
    fn a_stall_with_no_looks_does_not_count_as_running() {
        let start = Instant::now();
        let mut record = RunRecord::default();
        record.see(running(100, 10, 0), start);
        record.see(running(200, 10, 2000), start + Duration::from_secs(2));
        record.see(running(300, 10, 4000), start + Duration::from_secs(60));
        assert_eq!(record.ran(), Some(Duration::from_secs(2)));
    }

    #[test]
    fn a_file_started_over_forgets_its_peak_and_curve_of_unknown_size() {
        let start = Instant::now();
        let mut record = RunRecord::default();
        let unknown = |speed, done| Look {
            per_mille: None,
            ..running(0, speed, done)
        };
        record.see(unknown(90, 5000), start);
        record.see(unknown(10, 100), start + SAMPLE_EVERY);
        assert_eq!(record.peak(), Some(10));
        assert_eq!(record.points(1.), [(0., 10), (1., 10)]);
    }

    #[test]
    fn a_paused_download_is_not_stalled() {
        let start = Instant::now();
        let mut record = RunRecord::default();
        record.see(running(100, 10, 1000), start);
        record.see(
            Look {
                running: false,
                ..running(100, 0, 1000)
            },
            start + SAMPLE_EVERY,
        );
        assert!(!record.stalled(start + STALLED_AFTER * 2));
    }

    #[test]
    fn a_window_that_watched_nothing_has_nothing_to_say() {
        let record = RunRecord::default();
        assert!(!record.drawable());
        assert_eq!(record.points(1.), Vec::new());
        assert_eq!(record.ran(), None);
        assert_eq!(record.average(), None);
        assert_eq!(record.peak(), None);
    }
}
