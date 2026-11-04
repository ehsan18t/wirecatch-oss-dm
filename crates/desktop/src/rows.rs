//! The downloads as the windows keep them, in the manager's order, with what a window asks on
//! every progress tick kept ready, so a tick costs what changed and not the whole list.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::ops::Deref;

use wirecatch_protocol::app::{DownloadStatus, DownloadSummary, ProcessingStep};
use wirecatch_protocol::{DownloadId, QueueId};

use crate::main_window::Group;

/// Counts and sums over a set of downloads.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    pub count: usize,
    groups: [usize; Group::ALL.len()],
    /// How fast what runs moves.
    pub speed: u64,
    /// The sizes of what runs and waits, when known.
    pub size: u64,
    /// How much of `size` is done.
    pub done: u64,
    /// How many of what runs and waits have no known size.
    pub unsized_rows: usize,
    /// How many of what runs are merging or converting.
    pub joining: usize,
    pub running: BTreeSet<DownloadId>,
}

/// The tally of a queue that holds no downloads.
static EMPTY: Tally = Tally {
    count: 0,
    groups: [0; Group::ALL.len()],
    speed: 0,
    size: 0,
    done: 0,
    unsized_rows: 0,
    joining: 0,
    running: BTreeSet::new(),
};

/// What one row adds to a tally beyond its counts.
struct Share {
    speed: u64,
    size: u64,
    done: u64,
    unsized_rows: usize,
    joining: usize,
}

impl Share {
    fn of(row: &DownloadSummary) -> Self {
        let group = Group::of(row.status);
        let running = group == Group::Downloading;
        let sized = (running || group == Group::UpNext).then_some(row.total);
        Self {
            speed: if running { row.speed } else { 0 },
            size: sized.flatten().unwrap_or(0),
            done: sized.flatten().map_or(0, |total| row.done.min(total)),
            unsized_rows: usize::from(sized == Some(None)),
            joining: usize::from(
                running
                    && (row.status == DownloadStatus::Processing
                        || row
                            .processing
                            .as_ref()
                            .is_some_and(|p| p.step == ProcessingStep::Joining)),
            ),
        }
    }
}

impl Tally {
    pub fn in_group(&self, group: Group) -> usize {
        self.groups[group.place()]
    }

    fn add(&mut self, row: &DownloadSummary) {
        let share = Share::of(row);
        let group = Group::of(row.status);
        self.count += 1;
        self.groups[group.place()] += 1;
        self.speed += share.speed;
        self.size += share.size;
        self.done += share.done;
        self.unsized_rows += share.unsized_rows;
        self.joining += share.joining;
        if group == Group::Downloading {
            self.running.insert(row.id);
        }
    }

    /// Takes back exactly what [`Self::add`] added for `row`.
    fn take(&mut self, row: &DownloadSummary) {
        let share = Share::of(row);
        self.count -= 1;
        self.groups[Group::of(row.status).place()] -= 1;
        self.speed -= share.speed;
        self.size -= share.size;
        self.done -= share.done;
        self.unsized_rows -= share.unsized_rows;
        self.joining -= share.joining;
        self.running.remove(&row.id);
    }
}

/// The download list. It reads as a slice; it changes only through its methods, which keep the
/// rest in step.
#[derive(Default)]
pub struct Rows {
    list: Vec<DownloadSummary>,
    places: HashMap<DownloadId, usize>,
    all: Tally,
    queues: HashMap<QueueId, Tally>,
    revision: u64,
}

impl Deref for Rows {
    type Target = [DownloadSummary];

    fn deref(&self) -> &Self::Target {
        &self.list
    }
}

impl Rows {
    /// Starts over from the manager's full list.
    pub fn replace(&mut self, list: Vec<DownloadSummary>) {
        self.list = list;
        self.all = Tally::default();
        self.queues.clear();
        for row in &self.list {
            self.all.add(row);
            self.queues.entry(row.queue).or_default().add(row);
        }
        self.index();
        self.revision += 1;
    }

    /// Takes in `row`, as new or in place of the one with its id.
    pub fn put(&mut self, row: DownloadSummary) {
        let Some(place) = self.places.get(&row.id).copied() else {
            self.places.insert(row.id, self.list.len());
            self.count_in(&row);
            self.list.push(row);
            self.revision += 1;
            return;
        };
        let known = self.list[place].clone();
        if !sits_alike(&known, &row) {
            self.revision += 1;
        }
        self.count_out(&known);
        self.count_in(&row);
        self.list[place] = row;
    }

    pub fn remove(&mut self, ids: &[DownloadId]) {
        let gone: HashSet<DownloadId> = ids.iter().copied().collect();
        let (kept, removed): (Vec<_>, Vec<_>) = std::mem::take(&mut self.list)
            .into_iter()
            .partition(|row| !gone.contains(&row.id));
        self.list = kept;
        for row in &removed {
            self.count_out(row);
        }
        self.index();
        self.revision += 1;
    }

    /// Puts `ids_in_order` in that order among the places they hold.
    pub fn reorder(&mut self, ids_in_order: &[DownloadId]) {
        let mut places: Vec<usize> = ids_in_order
            .iter()
            .filter_map(|id| self.places.get(id).copied())
            .collect();
        let rows: Vec<DownloadSummary> = places.iter().map(|at| self.list[*at].clone()).collect();
        places.sort_unstable();
        for (place, row) in places.into_iter().zip(rows) {
            self.places.insert(row.id, place);
            self.list[place] = row;
        }
        self.revision += 1;
    }

    pub fn find(&self, id: DownloadId) -> Option<&DownloadSummary> {
        self.list.get(*self.places.get(&id)?)
    }

    /// Every download's tally.
    pub fn tally(&self) -> &Tally {
        &self.all
    }

    pub fn tally_of(&self, queue: QueueId) -> &Tally {
        self.queues.get(&queue).unwrap_or(&EMPTY)
    }

    /// The tally of each queue that holds downloads.
    pub fn queue_tallies(&self) -> impl Iterator<Item = (QueueId, &Tally)> {
        self.queues.iter().map(|(queue, tally)| (*queue, tally))
    }

    /// Moves whenever the list's grouping or order may differ; a progress tick leaves it.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    fn index(&mut self) {
        self.places = self
            .list
            .iter()
            .enumerate()
            .map(|(place, row)| (row.id, place))
            .collect();
    }

    fn count_in(&mut self, row: &DownloadSummary) {
        self.all.add(row);
        self.queues.entry(row.queue).or_default().add(row);
    }

    fn count_out(&mut self, row: &DownloadSummary) {
        self.all.take(row);
        if let Some(tally) = self.queues.get_mut(&row.queue) {
            tally.take(row);
            if tally.count == 0 {
                self.queues.remove(&row.queue);
            }
        }
    }
}

/// Whether two states of a row group, sort and search alike. What runs moves only its figures; a
/// row at rest shows `done` in its day's total.
fn sits_alike(a: &DownloadSummary, b: &DownloadSummary) -> bool {
    a.status == b.status
        && a.queue == b.queue
        && a.file_name == b.file_name
        && a.url == b.url
        && a.total == b.total
        && a.completed_at == b.completed_at
        && (Group::of(a.status) == Group::Downloading || a.done == b.done)
}

#[cfg(test)]
impl<'a> FromIterator<&'a DownloadSummary> for Tally {
    fn from_iter<I: IntoIterator<Item = &'a DownloadSummary>>(rows: I) -> Self {
        let mut tally = Self::default();
        for row in rows {
            tally.add(row);
        }
        tally
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wirecatch_test_support::downloads::download;

    fn row(id: i64, queue: i64, status: DownloadStatus, speed: u64) -> DownloadSummary {
        DownloadSummary {
            queue: QueueId(queue),
            status,
            speed,
            total: Some(100),
            done: 10,
            ..download(id)
        }
    }

    fn rows(list: Vec<DownloadSummary>) -> Rows {
        let mut rows = Rows::default();
        rows.replace(list);
        rows
    }

    /// Every kept figure equals a fresh count of the list.
    fn assert_in_step(rows: &Rows) {
        assert_eq!(*rows.tally(), rows.iter().collect::<Tally>());
        let queues: BTreeSet<QueueId> = rows.iter().map(|row| row.queue).collect();
        assert_eq!(rows.queue_tallies().count(), queues.len());
        for queue in queues {
            let fresh: Tally = rows.iter().filter(|row| row.queue == queue).collect();
            assert_eq!(*rows.tally_of(queue), fresh);
        }
        for (place, row) in rows.iter().enumerate() {
            assert_eq!(rows.places.get(&row.id), Some(&place));
        }
    }

    #[test]
    fn the_figures_stay_in_step_through_every_change() {
        use DownloadStatus::{Completed, Downloading, Failed, Paused, Processing, Queued};
        let mut rows = rows(vec![
            row(1, 1, Downloading, 500),
            row(2, 1, Queued, 0),
            row(3, 2, Completed, 0),
        ]);
        assert_in_step(&rows);
        rows.put(row(1, 1, Downloading, 900));
        assert_in_step(&rows);
        assert_eq!(rows.tally().speed, 900);
        rows.put(row(2, 2, Processing, 0));
        assert_in_step(&rows);
        assert_eq!(rows.tally().joining, 1);
        rows.put(DownloadSummary {
            total: None,
            ..row(4, 3, Queued, 0)
        });
        assert_in_step(&rows);
        assert_eq!(rows.tally_of(QueueId(3)).unsized_rows, 1);
        rows.put(row(4, 3, Failed, 0));
        assert_in_step(&rows);
        assert_eq!(rows.tally_of(QueueId(3)).unsized_rows, 0);
        rows.reorder(&[DownloadId(3), DownloadId(1)]);
        assert_in_step(&rows);
        rows.remove(&[DownloadId(4), DownloadId(2)]);
        assert_in_step(&rows);
        assert_eq!(*rows.tally_of(QueueId(3)), Tally::default());
        rows.put(row(1, 1, Paused, 0));
        assert_in_step(&rows);
        assert!(rows.tally().running.is_empty());
    }

    #[test]
    fn a_row_is_found_by_its_id_after_its_place_changed() {
        let mut rows = rows(vec![
            row(1, 1, DownloadStatus::Queued, 0),
            row(2, 1, DownloadStatus::Queued, 0),
            row(3, 1, DownloadStatus::Queued, 0),
        ]);
        rows.remove(&[DownloadId(1)]);
        rows.reorder(&[DownloadId(3), DownloadId(2)]);
        let order: Vec<i64> = rows.iter().map(|row| row.id.0).collect();
        assert_eq!(order, [3, 2]);
        assert_eq!(rows.find(DownloadId(2)).map(|row| row.id.0), Some(2));
        assert!(rows.find(DownloadId(1)).is_none());
    }

    #[test]
    fn a_progress_tick_leaves_the_revision_and_a_change_of_place_moves_it() {
        let mut rows = rows(vec![row(1, 1, DownloadStatus::Downloading, 500)]);
        let start = rows.revision();
        rows.put(DownloadSummary {
            done: 60,
            eta_secs: Some(3),
            ..row(1, 1, DownloadStatus::Downloading, 900)
        });
        assert_eq!(rows.revision(), start);
        rows.put(row(1, 1, DownloadStatus::Paused, 0));
        assert_ne!(rows.revision(), start);
        let paused = rows.revision();
        rows.put(row(2, 1, DownloadStatus::Queued, 0));
        assert_ne!(rows.revision(), paused);
    }

    #[test]
    fn a_queue_without_downloads_has_an_empty_tally() {
        assert_eq!(*rows(Vec::new()).tally_of(QueueId(9)), Tally::default());
    }
}
