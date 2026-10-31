//! The order of the downloads in each group. Up next always runs in queue order, and a queue's
//! line keeps the order it runs in. A sort by progress, speed or time left is taken when it is
//! chosen and whenever the list's downloads change, so rows never jump on a progress tick.

use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap};

use wirecatch_protocol::DownloadId;
use wirecatch_protocol::app::DownloadSummary;

use super::Group;

/// What the list sorts by.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Key {
    /// Each download where its queue runs it; finished work newest first.
    #[default]
    QueueOrder,
    Name,
    Progress,
    Size,
    Speed,
    TimeLeft,
}

impl Key {
    pub const ALL: [Self; 6] = [
        Self::QueueOrder,
        Self::Name,
        Self::Progress,
        Self::Size,
        Self::Speed,
        Self::TimeLeft,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::QueueOrder => "Queue order",
            Self::Name => "Name",
            Self::Progress => "Progress",
            Self::Size => "Size",
            Self::Speed => "Speed",
            Self::TimeLeft => "Time left",
        }
    }

    /// Whether its values move on every progress tick.
    fn is_live(self) -> bool {
        matches!(self, Self::Progress | Self::Speed | Self::TimeLeft)
    }
}

/// The chosen sort.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Sort {
    pub key: Key,
    pub descending: bool,
}

/// A live sort's order, as it was when taken.
#[derive(Default)]
pub struct Frozen {
    sort: Option<Sort>,
    ids: BTreeSet<DownloadId>,
    ranks: HashMap<DownloadId, usize>,
}

impl Sort {
    /// Puts `members` of `group` (indexes into `downloads`) in this order.
    pub fn apply(
        self,
        group: Group,
        members: &mut [usize],
        downloads: &[DownloadSummary],
        frozen: &mut Frozen,
    ) {
        if group == Group::UpNext {
            return;
        }
        if self.key == Key::QueueOrder {
            if group.is_over() {
                members.sort_by_key(|index| std::cmp::Reverse(downloads[*index].completed_at));
            }
            if self.descending {
                members.reverse();
            }
            return;
        }
        if self.key.is_live() {
            let ranks = frozen.ranks(self, downloads);
            members.sort_by_key(|index| ranks.get(&downloads[*index].id).copied());
            return;
        }
        members.sort_by(|a, b| self.compare(&downloads[*a], &downloads[*b]));
    }

    /// Orders two downloads; a download without a value goes last either way.
    fn compare(self, a: &DownloadSummary, b: &DownloadSummary) -> Ordering {
        let order = match self.key {
            Key::QueueOrder => Some(Ordering::Equal),
            Key::Name => Some(a.file_name.to_lowercase().cmp(&b.file_name.to_lowercase())),
            Key::Progress => known(progress(a), progress(b)),
            Key::Size => known(a.total, b.total),
            Key::Speed => known(moving(a).map(|r| r.speed), moving(b).map(|r| r.speed)),
            Key::TimeLeft => known(
                moving(a).and_then(|r| r.eta_secs),
                moving(b).and_then(|r| r.eta_secs),
            ),
        };
        match order {
            Some(order) if self.descending => order.reverse(),
            Some(order) => order,
            None => known_last(self, a, b),
        }
    }
}

/// How far a download is, as done over total, when its total is known.
fn progress(row: &DownloadSummary) -> Option<Fraction> {
    row.total
        .filter(|total| *total > 0)
        .map(|total| Fraction(row.done, total))
}

/// A download that moves data now.
fn moving(row: &DownloadSummary) -> Option<&DownloadSummary> {
    (row.speed > 0).then_some(row)
}

/// The order of two values when both are known; `None` when one is missing.
fn known<T: Ord>(a: Option<T>, b: Option<T>) -> Option<Ordering> {
    Some(a?.cmp(&b?))
}

/// With a value missing, the download that has one goes first.
fn known_last(sort: Sort, a: &DownloadSummary, b: &DownloadSummary) -> Ordering {
    let has = |row: &DownloadSummary| match sort.key {
        Key::QueueOrder | Key::Name => true,
        Key::Progress => progress(row).is_some(),
        Key::Size => row.total.is_some(),
        Key::Speed => moving(row).is_some(),
        Key::TimeLeft => moving(row).and_then(|r| r.eta_secs).is_some(),
    };
    has(b).cmp(&has(a))
}

/// `done` over `total`, compared exactly.
#[derive(PartialEq, Eq)]
struct Fraction(u64, u64);

impl Ord for Fraction {
    fn cmp(&self, other: &Self) -> Ordering {
        (u128::from(self.0) * u128::from(other.1)).cmp(&(u128::from(other.0) * u128::from(self.1)))
    }
}

impl PartialOrd for Fraction {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Frozen {
    /// The ranks of `sort` over `downloads`, taken again when the sort or the downloads changed.
    fn ranks(&mut self, sort: Sort, downloads: &[DownloadSummary]) -> &HashMap<DownloadId, usize> {
        let ids: BTreeSet<DownloadId> = downloads.iter().map(|row| row.id).collect();
        if self.sort != Some(sort) || self.ids != ids {
            let mut order: Vec<&DownloadSummary> = downloads.iter().collect();
            order.sort_by(|a, b| sort.compare(a, b));
            self.ranks = order
                .iter()
                .enumerate()
                .map(|(rank, row)| (row.id, rank))
                .collect();
            self.sort = Some(sort);
            self.ids = ids;
        }
        &self.ranks
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wirecatch_protocol::app::DownloadStatus;

    fn row(id: i64, name: &str, total: Option<u64>, done: u64) -> DownloadSummary {
        let mut row = wirecatch_test_support::downloads::download(id);
        row.file_name = name.to_owned();
        row.total = total;
        row.done = done;
        row.status = DownloadStatus::Paused;
        row
    }

    fn sorted(sort: Sort, downloads: &[DownloadSummary]) -> Vec<i64> {
        let mut members: Vec<usize> = (0..downloads.len()).collect();
        sort.apply(
            Group::Paused,
            &mut members,
            downloads,
            &mut Frozen::default(),
        );
        members.iter().map(|index| downloads[*index].id.0).collect()
    }

    #[test]
    fn a_download_without_a_value_goes_last_either_way() {
        let downloads = [
            row(1, "b", None, 0),
            row(2, "a", Some(100), 50),
            row(3, "c", Some(100), 10),
        ];
        let ascending = Sort {
            key: Key::Progress,
            descending: false,
        };
        let descending = Sort {
            key: Key::Progress,
            descending: true,
        };
        assert_eq!(sorted(ascending, &downloads), [3, 2, 1]);
        assert_eq!(sorted(descending, &downloads), [2, 3, 1]);
        let by_name = Sort {
            key: Key::Name,
            descending: false,
        };
        assert_eq!(sorted(by_name, &downloads), [2, 1, 3]);
    }
}
