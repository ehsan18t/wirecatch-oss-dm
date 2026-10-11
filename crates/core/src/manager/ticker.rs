//! Progress sampling and the batched `DownloadsChanged` event.
//!
//! While anything runs, the ticker reads each run's engine counters ten times a
//! second, smooths the speed and sends the rows that changed in one event. When
//! nothing runs it sleeps until a row changes. Either way at most one event goes out
//! per tick.

use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use tokio::sync::Notify;
use wirecatch_engine::CancellationToken;
use wirecatch_protocol::app::{DownloadStatus, DownloadSummary, UiEvent};

use super::state::{Entry, Gauge};
use super::{Inner, Manager};
use crate::speed;

/// Time between two `DownloadsChanged` events, and between progress samples.
pub(super) const TICK: Duration = Duration::from_millis(100);
/// How often the progress of running downloads is written to the database.
const SAVE_PROGRESS_EVERY: Duration = Duration::from_secs(2);

pub(super) async fn run(weak: Weak<Inner>, rows_changed: Arc<Notify>, stopped: CancellationToken) {
    let mut last_saved = Instant::now();
    let mut busy = true;
    loop {
        if !busy {
            tokio::select! {
                _ = rows_changed.notified() => {}
                _ = stopped.cancelled() => return,
            }
        }
        busy = match Manager::from_weak(&weak) {
            Some(manager) => manager.tick(&mut last_saved),
            None => return,
        };
        tokio::select! {
            _ = tokio::time::sleep(TICK) => {}
            _ = stopped.cancelled() => return,
        }
    }
}

impl Manager {
    /// Samples running downloads and sends changed rows. Returns whether anything runs.
    fn tick(&self, last_saved: &mut Instant) -> bool {
        let now = Instant::now();
        let mut state = self.lock();
        let state = &mut *state;
        #[cfg(debug_assertions)]
        state.assert_consistent();
        let mut to_mark = Vec::new();
        for id in &state.running {
            if let Some(entry) = state.downloads.get_mut(id) {
                if sample(entry, now) {
                    state.dirty.insert(*id);
                }
                to_mark.extend(part_to_mark(entry));
            }
        }
        if !to_mark.is_empty() {
            // The part file is marked as soon as it exists, so the finished file
            // carries the mark from the moment it is renamed into place.
            self.inner.runtime.spawn_blocking(move || {
                for (part, url, referrer) in to_mark {
                    crate::zone::mark(&part, &url, referrer.as_deref());
                }
            });
        }
        let running = !state.running.is_empty();

        if running && now.duration_since(*last_saved) >= SAVE_PROGRESS_EVERY {
            let rows = state
                .running
                .iter()
                .filter_map(|id| state.downloads.get(id))
                .map(Entry::stored)
                .collect();
            self.inner.store.save_downloads(rows);
            *last_saved = now;
        }

        let changed: Vec<DownloadSummary> = std::mem::take(&mut state.dirty)
            .into_iter()
            .filter_map(|id| state.entry(id).ok().map(|e| e.summary()))
            .collect();
        if !changed.is_empty() {
            self.emit(UiEvent::DownloadsChanged(changed));
        }
        running
    }
}

/// The engine's part file of a running download that has not been marked yet,
/// once the server has answered (the path is final then), with the addresses
/// for the mark.
fn part_to_mark(entry: &mut Entry) -> Option<(std::path::PathBuf, String, Option<String>)> {
    let run = entry.run.as_mut()?;
    if run.marked || !matches!(run.gauge, Gauge::Engine(_)) || !run.gauge.read().answered {
        return None;
    }
    run.marked = true;
    let [part, _] = crate::naming::partial_files(std::path::Path::new(&entry.row.path));
    let referrer = crate::headers::referrer(&entry.origin.headers).map(str::to_string);
    Some((part, entry.row.url.clone(), referrer))
}

/// Copies a run's engine counters into its row. Returns whether the row changed.
fn sample(entry: &mut Entry, now: Instant) -> bool {
    let Some(run) = &mut entry.run else {
        return false;
    };
    let row = &mut entry.row;
    let reading = run.gauge.read();
    // The engine names the file once the server has answered.
    if row.status == DownloadStatus::Connecting && !reading.answered {
        return false;
    }
    let before = (
        row.status,
        row.done,
        row.total,
        row.speed,
        row.connections,
        row.eta_secs,
    );
    row.status = if reading.processing {
        DownloadStatus::Processing
    } else {
        DownloadStatus::Downloading
    };
    row.done = reading.done;
    row.total = reading.total;
    row.connections = u32::try_from(reading.connections).unwrap_or(u32::MAX);
    row.speed = run.meter.sample(row.done, now);
    row.eta_secs = speed::eta(row.total, row.done, row.speed);
    before
        != (
            row.status,
            row.done,
            row.total,
            row.speed,
            row.connections,
            row.eta_secs,
        )
}
