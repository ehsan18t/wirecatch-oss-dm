//! What a download window saw of a download's run: its speed at each point of the file, how long
//! it really ran, and its average and peak. The app keeps no such history, so a window knows only
//! what it watched.

use std::time::{Duration, Instant};

use wirecatch_engine::numeric::{approx_f64, f64_to_u64};
use wirecatch_protocol::app::{DownloadStatus, DownloadSummary, PER_MILLE_DONE};

use crate::text;
use crate::ui::speed_graph::thousandths;

const SAMPLE_EVERY: Duration = Duration::from_millis(500);
/// Most samples kept: every other one goes when full, so a long run keeps its whole shape.
const MOST_SAMPLES: usize = 240;
/// Shorter than this, an average is mostly noise.
const SHORTEST_AVERAGE: Duration = Duration::from_secs(1);
/// The engine retries on its own and says nothing: this long with no bytes is all that shows it.
const STALLED_AFTER: Duration = Duration::from_secs(3);

/// One look at the running download.
#[derive(Clone, Copy, Debug)]
struct Look {
    /// How far it is, in tenths of a percent, when its size is known.
    per_mille: Option<u16>,
    speed: u64,
    done: u64,
    /// It is moving data now; time spent paused or waiting does not count.
    running: bool,
}

#[derive(Clone, Copy, Debug)]
struct Sample {
    done: u64,
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
    reached: Option<u16>,
    peak: u64,
    finished_average: Option<u64>,
}

impl RunRecord {
    pub fn watch(&mut self, row: &DownloadSummary, now: Instant) {
        if row.status == DownloadStatus::Completed {
            self.finish(row.done, now);
            return;
        }
        self.see(
            Look {
                per_mille: text::per_mille(row),
                speed: row.speed,
                done: row.done,
                running: row.status == DownloadStatus::Downloading,
            },
            now,
        );
    }

    fn see(&mut self, look: Look, now: Instant) {
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
        self.reached = look.per_mille;
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
            done: look.done,
            speed: look.speed,
        });
    }

    /// A single observed speed draws a flat line across the graph.
    pub fn drawable(&self) -> bool {
        !self.samples.is_empty() || self.finished_average.is_some()
    }

    /// How far across the run got, when its size is known.
    pub fn reached(&self) -> Option<f32> {
        self.reached
            .map(|at| f32::from(at) / f32::from(PER_MILLE_DONE))
    }

    /// Each sample as how far across (0 to 1) and its speed: across the file when its size is
    /// known, else across the bytes watched. Runs from the left edge to `end`, the nearest
    /// sample's speed filling what the window did not watch.
    pub fn points(&self, end: f32) -> Vec<(f32, u64)> {
        if let Some(speed) = self.finished_average {
            return vec![(0., speed), (end, speed)];
        }
        let from = match self.reached {
            Some(_) => 0,
            None => self.samples.first().map_or(0, |sample| sample.done),
        };
        let span = self.last_done.saturating_sub(from).max(1);
        // Placed by bytes, which only rise in a run; the percent falls when an estimated size grows.
        let mut points: Vec<(f32, u64)> = self
            .samples
            .iter()
            .map(|sample| {
                let part = u128::from(sample.done.saturating_sub(from));
                (thousandths(part, u128::from(span)) * end, sample.speed)
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

    /// Speed over the watched interval when the transfer finished without a speed sample.
    pub fn finished_average(&self) -> Option<u64> {
        self.finished_average
    }

    fn finish(&mut self, done: u64, now: Instant) {
        let last = self.last_seen_running.take();
        let stalled = self.stalled(now);
        self.last_moved = None;
        if !self.samples.is_empty() || stalled {
            return;
        }
        let (Some(first), Some(last)) = (self.first_done, last) else {
            return;
        };
        let watched = self.ran + now.saturating_duration_since(last);
        let speed = approx_f64(done.saturating_sub(first)) / watched.as_secs_f64();
        self.finished_average = f64_to_u64(speed).filter(|&speed| speed > 0);
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
        record.see(running(100, 10, 100), start);
        record.see(running(150, 90, 150), start + Duration::from_millis(100));
        record.see(running(200, 20, 200), start + SAMPLE_EVERY);
        assert_eq!(record.points(0.2), [(0., 10), (0.1, 10), (0.2, 20)]);
        assert_eq!(record.peak(), Some(90), "the peak sees every look");
    }

    #[test]
    fn one_observed_speed_draws_a_finished_runs_curve() {
        let mut record = RunRecord::default();
        record.see(running(600, 2400, 600), Instant::now());

        assert!(record.drawable());
        assert_eq!(record.points(1.), [(0., 2400), (1., 2400)]);
    }

    #[test]
    fn a_quick_finish_between_speed_samples_draws_the_observed_average() {
        let start = Instant::now();
        let mut record = RunRecord::default();
        let mut row = DownloadSummary {
            status: DownloadStatus::Downloading,
            speed: 0,
            done: 1000,
            total: Some(2000),
            ..wirecatch_test_support::downloads::download(1)
        };
        record.watch(&row, start);
        row.status = DownloadStatus::Completed;
        row.done = 2000;
        record.watch(&row, start + Duration::from_millis(100));

        assert!(record.drawable());
        assert_eq!(record.points(1.), [(0., 10_000), (1., 10_000)]);
        assert_eq!(record.peak(), None);
        assert_eq!(record.finished_average(), Some(10_000));
    }

    #[test]
    fn a_window_first_opened_after_completion_has_no_speed_to_plot() {
        let row = DownloadSummary {
            status: DownloadStatus::Completed,
            done: 2000,
            ..wirecatch_test_support::downloads::download(1)
        };
        let mut record = RunRecord::default();
        record.watch(&row, Instant::now());

        assert!(!record.drawable());
        assert_eq!(record.finished_average(), None);
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
    fn a_moving_size_estimate_never_turns_the_curve_back() {
        let start = Instant::now();
        let mut record = RunRecord::default();
        let unsized_yet = Look {
            per_mille: None,
            ..running(0, 10, 1000)
        };
        let looks = [
            unsized_yet,
            Look {
                done: 2000,
                ..unsized_yet
            },
            Look {
                done: 3000,
                ..unsized_yet
            },
            running(300, 20, 4000),
            running(300, 30, 5000),
            running(150, 40, 6000),
            running(200, 50, 8000),
        ];
        for (step, look) in (0u32..).zip(looks) {
            record.see(look, start + SAMPLE_EVERY * step);
        }
        let points = record.points(0.2);
        assert!(
            points.windows(2).all(|pair| pair[0].0 < pair[1].0),
            "{points:?}"
        );
        assert_eq!(points.last(), Some(&(0.2, 50)));
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
    fn an_unknown_size_spreads_across_the_bytes_watched() {
        let start = Instant::now();
        let mut record = RunRecord::default();
        for step in 0..3u32 {
            record.see(
                Look {
                    per_mille: None,
                    ..running(0, 10, 1000 + u64::from(step))
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
        record.see(running(100, 10, 1000), start);
        record.see(running(200, 30, 2000), start + SAMPLE_EVERY);
        record.see(
            running(400, 30, 4000),
            start + SAMPLE_EVERY + Duration::from_millis(100),
        );
        assert_eq!(
            record.points(0.8),
            [(0., 10), (0.2, 10), (0.4, 30), (0.8, 30)]
        );
        assert_eq!(record.points(1.).last(), Some(&(1., 30)));
    }

    #[test]
    fn the_last_look_says_how_far_the_run_reached() {
        let start = Instant::now();
        let mut record = RunRecord::default();
        assert_eq!(record.reached(), None);
        record.see(running(500, 10, 5000), start);
        assert_eq!(record.reached(), Some(0.5));
        record.see(
            Look {
                running: false,
                ..running(0, 0, 0)
            },
            start + SAMPLE_EVERY,
        );
        assert_eq!(record.reached(), Some(0.5), "only running looks count");
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
