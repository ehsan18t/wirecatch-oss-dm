//! What the download complete window lists (D52.6): downloads told one by one share a group,
//! a queue's summary is a group under its own heading, newest first, within
//! [`MAX_FINISHED_GROUPS`] groups of [`MAX_FINISHED_LISTED`] downloads.

use wirecatch_protocol::app::{DownloadStatus, DownloadSummary, UiEvent};
use wirecatch_protocol::desktop::{
    FinishedGroup, FinishedList, FinishedQueue, MAX_FINISHED_GROUPS, MAX_FINISHED_LISTED,
};

/// What the download complete window lists (D52.6).
#[derive(Debug, Default)]
pub struct FinishedBook {
    list: FinishedList,
}

impl FinishedBook {
    /// Adds a group, newest first: downloads told one by one join the newest group when it is
    /// one of those too. A download listed before (it failed, was tried again and finished)
    /// leaves its old place. Returns whether anything was added.
    pub fn add(&mut self, queue: Option<FinishedQueue>, ended: &[DownloadSummary]) -> bool {
        if ended.is_empty() {
            return false;
        }
        self.update_rows(|listed| !ended.iter().any(|e| e.id == listed.id));
        let newest_first = ended.iter().rev().cloned();
        match self.list.groups.first_mut() {
            Some(top) if queue.is_none() && top.queue.is_none() => {
                let mut downloads: Vec<_> = newest_first.collect();
                downloads.append(&mut top.downloads);
                downloads.truncate(MAX_FINISHED_LISTED);
                top.downloads = downloads;
            }
            _ => {
                self.list.groups.insert(
                    0,
                    FinishedGroup {
                        queue,
                        downloads: newest_first.take(MAX_FINISHED_LISTED).collect(),
                    },
                );
                self.list.groups.truncate(MAX_FINISHED_GROUPS);
            }
        }
        true
    }

    /// Follows the rows it lists: a change replaces the row, and a download removed from the
    /// list leaves it, so a newer list sent to the window never brings back an old row.
    pub fn follow(&mut self, event: &UiEvent) {
        if self.list.groups.is_empty() {
            return;
        }
        match event {
            UiEvent::DownloadsChanged(rows) => {
                self.update_rows(|listed| {
                    if let Some(row) = rows.iter().find(|r| r.id == listed.id) {
                        listed.clone_from(row);
                    }
                    true
                });
            }
            UiEvent::DownloadsRemoved(ids) => {
                self.update_rows(|listed| !ids.contains(&listed.id));
            }
            _ => {}
        }
    }

    /// Brings the rows it lists up to date with `rows`, the whole list after missed events.
    pub fn reload(&mut self, rows: &[DownloadSummary]) {
        self.update_rows(|listed| {
            let row = rows.iter().find(|row| row.id == listed.id);
            if let Some(row) = row {
                listed.clone_from(row);
            }
            row.is_some()
        });
    }

    /// Runs `update` on every listed row, dropping those it returns false for and the groups left
    /// empty. Heading counts move by what changed: they also cover rows past
    /// [`MAX_FINISHED_LISTED`], so counting the listed rows again would lose those.
    fn update_rows(&mut self, mut update: impl FnMut(&mut DownloadSummary) -> bool) {
        for FinishedGroup { queue, downloads } in &mut self.list.groups {
            downloads.retain_mut(|row| {
                let before = row.status;
                let keep = update(row);
                if let Some(queue) = queue {
                    let after = keep.then_some(row.status);
                    if after != Some(before) {
                        if let Some(count) = heading_count(queue, before) {
                            *count = count.saturating_sub(1);
                        }
                        if let Some(count) = after.and_then(|status| heading_count(queue, status)) {
                            *count = count.saturating_add(1);
                        }
                    }
                }
                keep
            });
        }
        self.list.groups.retain(|g| !g.downloads.is_empty());
    }

    /// What the window lists now.
    #[must_use]
    pub fn list(&self) -> &FinishedList {
        &self.list
    }
}

/// The count on a queue's heading that a row of `status` belongs to; a row that has not ended has none.
fn heading_count(queue: &mut FinishedQueue, status: DownloadStatus) -> Option<&mut u32> {
    match status {
        DownloadStatus::Completed => Some(&mut queue.completed),
        DownloadStatus::Failed => Some(&mut queue.failed),
        _ => None,
    }
}

/// The summary header of a queue that is done.
#[must_use]
pub fn queue_header(
    id: wirecatch_protocol::QueueId,
    name: &str,
    ended: &[DownloadSummary],
) -> FinishedQueue {
    let count = |status| {
        u32::try_from(ended.iter().filter(|d| d.status == status).count()).unwrap_or(u32::MAX)
    };
    FinishedQueue {
        id,
        name: name.to_owned(),
        completed: count(DownloadStatus::Completed),
        failed: count(DownloadStatus::Failed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wirecatch_protocol::{DownloadId, QueueId};

    fn ended(id: i64, status: DownloadStatus) -> DownloadSummary {
        let mut row = wirecatch_test_support::downloads::download(id);
        row.status = status;
        row
    }

    fn ids(group: &FinishedGroup) -> Vec<i64> {
        group.downloads.iter().map(|d| d.id.0).collect()
    }

    #[test]
    fn a_reload_drops_removed_rows_and_takes_changed_ones() {
        let mut book = FinishedBook::default();
        book.add(
            None,
            &[
                ended(1, DownloadStatus::Completed),
                ended(2, DownloadStatus::Failed),
            ],
        );
        book.reload(&[
            ended(2, DownloadStatus::Completed),
            ended(3, DownloadStatus::Queued),
        ]);
        assert_eq!(ids(&book.list().groups[0]), [2]);
        assert_eq!(
            book.list().groups[0].downloads[0].status,
            DownloadStatus::Completed
        );
        book.reload(&[]);
        assert_eq!(book.list().groups.len(), 0);
    }

    #[test]
    fn downloads_told_one_by_one_join_the_newest_group_newest_first() {
        let mut book = FinishedBook::default();
        assert!(!book.add(None, &[]));
        assert!(book.add(None, &[ended(1, DownloadStatus::Completed)]));
        assert!(book.add(
            None,
            &[
                ended(2, DownloadStatus::Completed),
                ended(3, DownloadStatus::Failed)
            ]
        ));
        let [group] = book.list().groups.as_slice() else {
            panic!("{:?}", book.list());
        };
        assert_eq!(ids(group), [3, 2, 1]);
    }

    #[test]
    fn a_queue_summary_is_a_group_of_its_own_above_the_rest() {
        let mut book = FinishedBook::default();
        book.add(None, &[ended(1, DownloadStatus::Completed)]);
        let rows = [
            ended(2, DownloadStatus::Completed),
            ended(3, DownloadStatus::Failed),
        ];
        book.add(Some(queue_header(QueueId(4), "Night", &rows)), &rows);
        book.add(None, &[ended(5, DownloadStatus::Completed)]);
        let groups = &book.list().groups;
        assert_eq!(
            groups.iter().map(ids).collect::<Vec<_>>(),
            [vec![5], vec![3, 2], vec![1]]
        );
        let night = groups[1].queue.as_ref().unwrap();
        assert_eq!(
            (night.name.as_str(), night.completed, night.failed),
            ("Night", 1, 1)
        );
    }

    #[test]
    fn a_download_tried_again_moves_to_where_it_ended_last() {
        let mut book = FinishedBook::default();
        book.add(None, &[ended(1, DownloadStatus::Failed)]);
        let rows = [ended(1, DownloadStatus::Completed)];
        book.add(Some(queue_header(QueueId(4), "Night", &rows)), &rows);
        assert_eq!(
            book.list().groups.iter().map(ids).collect::<Vec<_>>(),
            [vec![1]]
        );
    }

    #[test]
    fn the_list_follows_what_happens_to_its_downloads() {
        let mut book = FinishedBook::default();
        book.add(
            None,
            &[
                ended(1, DownloadStatus::Failed),
                ended(2, DownloadStatus::Completed),
            ],
        );
        book.follow(&UiEvent::DownloadsChanged(vec![ended(
            1,
            DownloadStatus::Downloading,
        )]));
        assert_eq!(
            book.list().groups[0].downloads[1].status,
            DownloadStatus::Downloading
        );
        book.follow(&UiEvent::DownloadsRemoved(vec![
            DownloadId(1),
            DownloadId(2),
        ]));
        assert!(book.list().groups.is_empty(), "{:?}", book.list());
    }

    /// A queue's group: downloads 1 and 2 finished, 3 failed.
    fn night_group() -> FinishedBook {
        let rows = [
            ended(1, DownloadStatus::Completed),
            ended(2, DownloadStatus::Completed),
            ended(3, DownloadStatus::Failed),
        ];
        let mut book = FinishedBook::default();
        book.add(Some(queue_header(QueueId(4), "Night", &rows)), &rows);
        book
    }

    fn counts(book: &FinishedBook) -> (u32, u32) {
        let queue = book.list().groups[0].queue.as_ref().unwrap();
        (queue.completed, queue.failed)
    }

    #[test]
    fn a_groups_heading_counts_follow_its_rows() {
        let mut book = night_group();
        book.follow(&UiEvent::DownloadsChanged(vec![ended(
            3,
            DownloadStatus::Completed,
        )]));
        assert_eq!(counts(&book), (3, 0), "a failed download finished");
        book.follow(&UiEvent::DownloadsChanged(vec![ended(
            1,
            DownloadStatus::Downloading,
        )]));
        assert_eq!(counts(&book), (2, 0), "a finished download runs again");
        book.follow(&UiEvent::DownloadsRemoved(vec![DownloadId(2)]));
        assert_eq!(counts(&book), (1, 0), "a finished download was removed");
    }

    #[test]
    fn a_reload_brings_the_heading_counts_along() {
        let mut book = night_group();
        book.reload(&[
            ended(1, DownloadStatus::Failed),
            ended(3, DownloadStatus::Failed),
        ]);
        assert_eq!(counts(&book), (0, 2));
    }

    #[test]
    fn a_download_that_ends_again_leaves_its_old_groups_counts() {
        let mut book = night_group();
        book.add(None, &[ended(5, DownloadStatus::Completed)]);
        book.add(None, &[ended(3, DownloadStatus::Completed)]);
        let night = book.list().groups[1].queue.as_ref().unwrap();
        assert_eq!((night.completed, night.failed), (2, 0));
    }

    #[test]
    fn the_counts_keep_the_downloads_a_full_group_does_not_list() {
        let rows: Vec<_> = (0..150)
            .map(|n| ended(n, DownloadStatus::Completed))
            .collect();
        let mut book = FinishedBook::default();
        book.add(Some(queue_header(QueueId(4), "Night", &rows)), &rows);
        book.follow(&UiEvent::DownloadsRemoved(vec![DownloadId(149)]));
        assert_eq!(counts(&book), (149, 0));
    }

    #[test]
    fn the_window_keeps_a_bounded_list() {
        let mut book = FinishedBook::default();
        let many: Vec<_> = (0..300)
            .map(|n| ended(n, DownloadStatus::Completed))
            .collect();
        book.add(None, &many);
        assert_eq!(book.list().groups[0].downloads.len(), MAX_FINISHED_LISTED);
        for n in 0..50 {
            let rows = [ended(1000 + n, DownloadStatus::Completed)];
            book.add(Some(queue_header(QueueId(2), "Q", &rows)), &rows);
        }
        assert_eq!(book.list().groups.len(), MAX_FINISHED_GROUPS);
    }
}
