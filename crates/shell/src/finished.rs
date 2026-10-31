//! What the download complete window lists (D52.6): downloads told one by one share a group,
//! a queue's summary is a group under its own heading, newest first, within
//! [`MAX_FINISHED_GROUPS`] groups of [`MAX_FINISHED_LISTED`] downloads.

use wirecatch_protocol::DownloadId;
use wirecatch_protocol::app::{DownloadStatus, DownloadSummary, UiEvent};
use wirecatch_protocol::desktop::{
    FinishedGroup, FinishedList, FinishedQueue, MAX_FINISHED_GROUPS, MAX_FINISHED_LISTED,
};

/// What the download complete window lists (D52.6).
#[derive(Debug, Default)]
pub struct FinishedBook {
    list: FinishedList,
    next_key: u32,
}

impl FinishedBook {
    /// Adds a group, newest first: downloads told one by one join the newest group when it is
    /// one of those too. A download listed before (it failed, was tried again and finished)
    /// leaves its old place. Returns whether anything was added.
    pub fn add(&mut self, queue: Option<FinishedQueue>, ended: &[DownloadSummary]) -> bool {
        if ended.is_empty() {
            return false;
        }
        for group in &mut self.list.groups {
            group
                .downloads
                .retain(|d| !ended.iter().any(|e| e.id == d.id));
        }
        self.list.groups.retain(|g| !g.downloads.is_empty());
        let newest_first = ended.iter().rev().cloned();
        match self.list.groups.first_mut() {
            Some(top) if queue.is_none() && top.queue.is_none() => {
                let mut downloads: Vec<_> = newest_first.collect();
                downloads.append(&mut top.downloads);
                downloads.truncate(MAX_FINISHED_LISTED);
                top.downloads = downloads;
            }
            _ => {
                self.next_key = self.next_key.wrapping_add(1);
                self.list.groups.insert(
                    0,
                    FinishedGroup {
                        key: self.next_key,
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
                for listed in self
                    .list
                    .groups
                    .iter_mut()
                    .flat_map(|g| g.downloads.iter_mut())
                {
                    if let Some(row) = rows.iter().find(|r| r.id == listed.id) {
                        listed.clone_from(row);
                    }
                }
            }
            UiEvent::DownloadsRemoved(ids) => {
                for group in &mut self.list.groups {
                    group.downloads.retain(|d| !ids.contains(&d.id));
                }
                self.list.groups.retain(|g| !g.downloads.is_empty());
            }
            _ => {}
        }
    }

    /// What the window lists now.
    #[must_use]
    pub fn list(&self) -> &FinishedList {
        &self.list
    }

    /// Whether the window lists download `id`.
    #[must_use]
    pub fn lists(&self, id: DownloadId) -> bool {
        self.list
            .groups
            .iter()
            .any(|g| g.downloads.iter().any(|d| d.id == id))
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
    use wirecatch_protocol::QueueId;

    fn ended(id: i64, status: DownloadStatus) -> DownloadSummary {
        let mut row = wirecatch_test_support::downloads::download(id);
        row.status = status;
        row
    }

    fn ids(group: &FinishedGroup) -> Vec<i64> {
        group.downloads.iter().map(|d| d.id.0).collect()
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
        assert!(book.lists(DownloadId(2)) && !book.lists(DownloadId(4)));
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
        let keys: Vec<_> = groups.iter().map(|g| g.key).collect();
        assert_eq!(
            keys.len(),
            keys.iter().collect::<std::collections::BTreeSet<_>>().len()
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
