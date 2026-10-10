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

use super::state::Entry;
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
        let mut running = false;
        for entry in state.downloads.values_mut() {
            if entry.is_running() {
                running = true;
                if sample(entry, now) {
                    state.dirty.insert(entry.row.id);
                }
            }
        }

        if running && now.duration_since(*last_saved) >= SAVE_PROGRESS_EVERY {
            let rows = state
                .downloads
                .values()
                .filter(|e| e.is_running())
                .map(Entry::stored)
                .collect();
            self.inner.store.save_downloads(rows);
            *last_saved = now;
        }

        let changed: Vec<DownloadSummary> = std::mem::take(&mut state.dirty)
            .into_iter()
            .filter_map(|id| state.downloads.get(&id).map(|e| e.row.clone()))
            .collect();
        if !changed.is_empty() {
            self.emit(UiEvent::DownloadsChanged(changed));
        }
        running
    }
}

/// Copies a run's engine counters into its row. Returns whether the row changed.
fn sample(entry: &mut Entry, now: Instant) -> bool {
    let Some(run) = &mut entry.run else {
        return false;
    };
    let row = &mut entry.row;
    let progress = &run.progress;
    // The engine names the file once the server has answered.
    if row.status == DownloadStatus::Connecting && progress.file_name().is_none() {
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
    row.status = DownloadStatus::Downloading;
    row.done = progress.done();
    row.total = progress.total();
    row.connections = u32::try_from(progress.connections()).unwrap_or(u32::MAX);
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
