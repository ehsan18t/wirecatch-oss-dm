//! Which downloads finished or failed while each queue was at work, so a queue can say what it
//! did once it is done (D52).
//!
//! A queue is at work while any of its downloads downloads or waits to start, and done when
//! none does. Its batch holds what finished or failed in between. Only rows that changed are
//! looked at, so a progress tick costs nothing per download in the list.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use wirecatch_protocol::app::{DownloadStatus, DownloadSummary};
use wirecatch_protocol::{DownloadId, QueueId};
pub(crate) use wirecatch_records::batch::{Outcomes, StoredBatch};

/// Downloading, or waiting to start: the queue is not done while one of its downloads is.
/// A download the person paused does not hold its queue open, or one forgotten pause would
/// keep the queue from ever being done.
fn is_at_work(status: DownloadStatus) -> bool {
    matches!(
        status,
        DownloadStatus::Queued
            | DownloadStatus::Connecting
            | DownloadStatus::Downloading
            | DownloadStatus::Processing
    )
}

#[derive(Debug, Clone, Copy)]
struct Seen {
    queue: QueueId,
    status: DownloadStatus,
}

/// Every queue's batch, kept up to date from the rows that change.
#[derive(Debug, Default)]
pub(crate) struct Batches {
    /// Each download's queue and status, as last seen.
    seen: HashMap<DownloadId, Seen>,
    /// How many downloads of each queue are at work.
    at_work: HashMap<QueueId, usize>,
    /// The batches of queues that started working and are not done yet.
    open: BTreeMap<QueueId, Outcomes>,
    /// Queues that may have become done since [`Batches::settle`] last ran.
    to_settle: BTreeSet<QueueId>,
    /// The batches changed since they were last saved.
    unsaved: bool,
}

impl Batches {
    /// Starts from every download as it is now and the batches saved before a restart. A saved
    /// batch whose queue has nothing at work is dropped: the queue was done when the app
    /// stopped, and telling the person about it now, with nobody yet listening, would be lost.
    pub fn restore<'a>(
        rows: impl IntoIterator<Item = &'a DownloadSummary>,
        stored: Vec<StoredBatch>,
    ) -> Self {
        let mut batches = Self::default();
        for row in rows {
            batches.seen.insert(
                row.id,
                Seen {
                    queue: row.queue,
                    status: row.status,
                },
            );
            if is_at_work(row.status) {
                *batches.at_work.entry(row.queue).or_default() += 1;
                batches.open.entry(row.queue).or_default();
            }
        }
        for batch in stored {
            match batches.open.get_mut(&batch.queue) {
                Some(open) => *open = batch.outcomes,
                None => batches.unsaved = true,
            }
        }
        batches
    }

    /// Takes in a row that changed.
    pub fn observe(&mut self, row: &DownloadSummary) {
        let id = row.id;
        let before = self.seen.insert(
            id,
            Seen {
                queue: row.queue,
                status: row.status,
            },
        );
        let was_at_work = before.is_some_and(|b| is_at_work(b.status) && b.queue == row.queue);
        if let Some(before) = before {
            if is_at_work(before.status) {
                self.ease(before.queue);
            }
            if before.queue != row.queue {
                self.drop_outcome(before.queue, id);
            }
        }
        if is_at_work(row.status) {
            *self.at_work.entry(row.queue).or_default() += 1;
            self.open.entry(row.queue).or_default();
            // Tried again: it ends again later, and only that end counts.
            if !was_at_work {
                self.drop_outcome(row.queue, id);
            }
        }
        // Only a change of status ends a download: one that already ended and only moves to
        // another queue ended in its old queue's run, and starts no run in the new one.
        let ended_now = |status| before.is_none_or(|b| b.status != status);
        match row.status {
            DownloadStatus::Completed if ended_now(DownloadStatus::Completed) => {
                self.record(row.queue, id, |o| &mut o.completed);
            }
            DownloadStatus::Failed if ended_now(DownloadStatus::Failed) => {
                self.record(row.queue, id, |o| &mut o.failed);
            }
            DownloadStatus::Cancelled => self.drop_outcome(row.queue, id),
            _ => {}
        }
        self.to_settle.insert(row.queue);
    }

    /// A download left the list.
    pub fn forget(&mut self, id: DownloadId) {
        if let Some(seen) = self.seen.remove(&id) {
            if is_at_work(seen.status) {
                self.ease(seen.queue);
            }
            self.drop_outcome(seen.queue, id);
        }
    }

    /// A queue was deleted; its downloads moved elsewhere, and its batch goes.
    pub fn forget_queue(&mut self, queue: QueueId) {
        if self.open.remove(&queue).is_some_and(|o| !o.is_empty()) {
            self.unsaved = true;
        }
        self.at_work.remove(&queue);
        self.to_settle.remove(&queue);
    }

    /// Closes the batches of queues that are done now, returning those in which something
    /// finished or failed. A queue whose downloads were all cancelled says nothing.
    pub fn settle(&mut self) -> Vec<(QueueId, Outcomes)> {
        let mut done = Vec::new();
        for queue in std::mem::take(&mut self.to_settle) {
            if self.at_work.get(&queue).is_some_and(|n| *n > 0) {
                continue;
            }
            if let Some(outcomes) = self.open.remove(&queue).filter(|o| !o.is_empty()) {
                self.unsaved = true;
                done.push((queue, outcomes));
            }
        }
        done
    }

    /// The batches to save, when they changed since last time.
    pub fn take_unsaved(&mut self) -> Option<Vec<StoredBatch>> {
        if !std::mem::take(&mut self.unsaved) {
            return None;
        }
        Some(
            self.open
                .iter()
                .filter(|(_, outcomes)| !outcomes.is_empty())
                .map(|(queue, outcomes)| StoredBatch {
                    queue: *queue,
                    outcomes: outcomes.clone(),
                })
                .collect(),
        )
    }

    fn ease(&mut self, queue: QueueId) {
        if let Some(n) = self.at_work.get_mut(&queue) {
            *n = n.saturating_sub(1);
        }
        self.to_settle.insert(queue);
    }

    fn record(
        &mut self,
        queue: QueueId,
        id: DownloadId,
        list: fn(&mut Outcomes) -> &mut Vec<DownloadId>,
    ) {
        let outcomes = self.open.entry(queue).or_default();
        outcomes.drop_id(id);
        list(outcomes).push(id);
        self.unsaved = true;
    }

    fn drop_outcome(&mut self, queue: QueueId, id: DownloadId) {
        if self.open.get_mut(&queue).is_some_and(|o| o.drop_id(id)) {
            self.unsaved = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wirecatch_protocol::app::Category;

    const MAIN: QueueId = QueueId(1);
    const NIGHT: QueueId = QueueId(2);

    fn row(id: i64, queue: QueueId, status: DownloadStatus) -> DownloadSummary {
        DownloadSummary {
            id: DownloadId(id),
            url: String::new(),
            file_name: format!("file-{id}"),
            path: String::new(),
            queue,
            category: Category::Other,
            status,
            total: None,
            done: 0,
            speed: 0,
            connections: 0,
            eta_secs: None,
            error: None,
            waiting: None,
            media: None,
            added_at: 0,
            completed_at: None,
            processing: None,
        }
    }

    fn ids(list: &[i64]) -> Vec<DownloadId> {
        list.iter().copied().map(DownloadId).collect()
    }

    fn outcomes(completed: &[i64], failed: &[i64]) -> Outcomes {
        Outcomes {
            completed: ids(completed),
            failed: ids(failed),
        }
    }

    /// Feeds each step's rows, as one tick would, and returns what settled after the last.
    fn run(
        batches: &mut Batches,
        steps: &[&[(i64, QueueId, DownloadStatus)]],
    ) -> Vec<(QueueId, Outcomes)> {
        let mut settled = Vec::new();
        for step in steps {
            for (id, queue, status) in *step {
                batches.observe(&row(*id, *queue, *status));
            }
            settled = batches.settle();
        }
        settled
    }

    use DownloadStatus::{Cancelled, Completed, Downloading, Failed, Paused, Queued};

    #[test]
    fn a_queue_is_done_when_nothing_in_it_runs_or_waits() {
        let mut batches = Batches::default();
        let settled = run(
            &mut batches,
            &[
                &[(1, MAIN, Downloading), (2, MAIN, Queued)],
                &[(1, MAIN, Completed), (2, MAIN, Downloading)],
            ],
        );
        assert!(settled.is_empty(), "one is still downloading");
        let settled = run(&mut batches, &[&[(2, MAIN, Failed)]]);
        assert_eq!(settled, vec![(MAIN, outcomes(&[1], &[2]))]);
        assert!(run(&mut batches, &[&[]]).is_empty(), "told once");
    }

    #[test]
    fn a_paused_download_does_not_hold_its_queue_open() {
        let mut batches = Batches::default();
        let settled = run(
            &mut batches,
            &[
                &[(1, MAIN, Downloading), (2, MAIN, Downloading)],
                &[(2, MAIN, Paused)],
                &[(1, MAIN, Completed)],
            ],
        );
        assert_eq!(settled, vec![(MAIN, outcomes(&[1], &[]))]);
    }

    #[test]
    fn a_queue_waiting_for_its_schedule_is_not_done() {
        let mut batches = Batches::default();
        let settled = run(
            &mut batches,
            &[
                &[(1, MAIN, Downloading), (2, MAIN, Queued)],
                &[(1, MAIN, Completed)],
            ],
        );
        assert!(settled.is_empty(), "the queued one runs in the next window");
        let settled = run(
            &mut batches,
            &[&[(2, MAIN, Downloading)], &[(2, MAIN, Completed)]],
        );
        assert_eq!(settled, vec![(MAIN, outcomes(&[1, 2], &[]))]);
    }

    #[test]
    fn downloads_added_while_running_join_the_batch() {
        let mut batches = Batches::default();
        let settled = run(
            &mut batches,
            &[
                &[(1, MAIN, Downloading)],
                &[(2, MAIN, Queued)],
                &[(1, MAIN, Completed), (2, MAIN, Downloading)],
                &[(2, MAIN, Completed)],
            ],
        );
        assert_eq!(settled, vec![(MAIN, outcomes(&[1, 2], &[]))]);
    }

    #[test]
    fn a_queue_whose_downloads_were_all_cancelled_says_nothing() {
        let mut batches = Batches::default();
        let settled = run(
            &mut batches,
            &[
                &[(1, MAIN, Downloading), (2, MAIN, Failed)],
                &[(1, MAIN, Cancelled), (2, MAIN, Queued)],
                &[(2, MAIN, Cancelled)],
            ],
        );
        assert_eq!(settled, Vec::new());
    }

    #[test]
    fn a_download_tried_again_counts_only_how_it_ended_last() {
        let mut batches = Batches::default();
        let settled = run(
            &mut batches,
            &[
                &[(1, MAIN, Downloading), (2, MAIN, Downloading)],
                &[(1, MAIN, Failed)],
                &[(1, MAIN, Queued)],
                &[(1, MAIN, Completed), (2, MAIN, Completed)],
            ],
        );
        assert_eq!(settled, vec![(MAIN, outcomes(&[1, 2], &[]))]);
    }

    #[test]
    fn a_download_that_ends_between_two_ticks_still_counts() {
        let mut batches = Batches::default();
        let settled = run(&mut batches, &[&[(1, MAIN, Completed)]]);
        assert_eq!(settled, vec![(MAIN, outcomes(&[1], &[]))]);
    }

    #[test]
    fn a_finished_download_changing_again_is_not_counted_again() {
        let done = row(1, MAIN, Completed);
        let mut batches = Batches::restore([&done], Vec::new());
        let settled = run(
            &mut batches,
            &[
                &[(2, MAIN, Downloading)],
                &[(1, MAIN, Completed)],
                &[(2, MAIN, Completed)],
            ],
        );
        assert_eq!(settled, vec![(MAIN, outcomes(&[2], &[]))]);
    }

    #[test]
    fn each_queue_has_its_own_batch() {
        let mut batches = Batches::default();
        let settled = run(
            &mut batches,
            &[
                &[(1, MAIN, Downloading), (2, NIGHT, Queued)],
                &[(1, MAIN, Completed)],
            ],
        );
        assert_eq!(
            settled,
            vec![(MAIN, outcomes(&[1], &[]))],
            "the night queue still waits"
        );
    }

    #[test]
    fn removing_the_last_waiting_download_ends_the_batch() {
        let mut batches = Batches::default();
        run(
            &mut batches,
            &[
                &[(1, MAIN, Downloading), (2, MAIN, Queued)],
                &[(1, MAIN, Completed)],
            ],
        );
        batches.forget(DownloadId(2));
        assert_eq!(batches.settle(), vec![(MAIN, outcomes(&[1], &[]))]);
    }

    #[test]
    fn a_download_moved_to_another_queue_belongs_to_that_queue() {
        let mut batches = Batches::default();
        let settled = run(
            &mut batches,
            &[
                &[(1, MAIN, Downloading), (2, MAIN, Downloading)],
                &[(2, NIGHT, Queued)],
                &[(1, MAIN, Completed)],
            ],
        );
        assert_eq!(settled, vec![(MAIN, outcomes(&[1], &[]))]);
        let settled = run(&mut batches, &[&[(2, NIGHT, Completed)]]);
        assert_eq!(settled, vec![(NIGHT, outcomes(&[2], &[]))]);
    }

    #[test]
    fn moving_finished_downloads_to_another_queue_reports_nothing() {
        let rows = [row(1, NIGHT, Completed), row(2, NIGHT, Failed)];
        let mut batches = Batches::restore(&rows, Vec::new());
        let settled = run(&mut batches, &[&[(1, MAIN, Completed), (2, MAIN, Failed)]]);
        assert_eq!(settled, Vec::new());
    }

    #[test]
    fn a_batch_survives_a_restart() {
        let mut before = Batches::default();
        run(
            &mut before,
            &[
                &[(1, MAIN, Downloading), (2, MAIN, Downloading)],
                &[(1, MAIN, Completed)],
            ],
        );
        let stored = before
            .take_unsaved()
            .expect("the finished download is saved");
        assert!(before.take_unsaved().is_none(), "saved once");

        // The running download comes back queued.
        let rows = [row(1, MAIN, Completed), row(2, MAIN, Queued)];
        let mut after = Batches::restore(&rows, stored);
        let settled = run(&mut after, &[&[(2, MAIN, Completed)]]);
        assert_eq!(settled, vec![(MAIN, outcomes(&[1, 2], &[]))]);
    }

    #[test]
    fn a_saved_batch_of_a_queue_with_nothing_left_is_dropped() {
        let stored = vec![StoredBatch {
            queue: MAIN,
            outcomes: outcomes(&[1], &[]),
        }];
        let mut batches = Batches::restore(&[row(1, MAIN, Completed)], stored);
        assert_eq!(batches.take_unsaved(), Some(Vec::new()));
        assert_eq!(batches.settle(), Vec::new());
    }

    #[test]
    fn a_deleted_queue_takes_its_batch_with_it() {
        let mut batches = Batches::default();
        run(
            &mut batches,
            &[&[(1, NIGHT, Downloading), (2, NIGHT, Failed)]],
        );
        batches.forget_queue(NIGHT);
        let settled = run(&mut batches, &[&[(1, MAIN, Completed)]]);
        assert_eq!(settled, vec![(MAIN, outcomes(&[1], &[]))]);
    }
}
