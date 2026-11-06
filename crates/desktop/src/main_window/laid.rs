//! What the list lays out, kept until something it depends on changes: a redraw or a progress tick
//! reads the same lines again.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use chrono::NaiveDate;
use gpui_kit::{ListOffset, Pixels};
use wirecatch_protocol::DownloadId;
use wirecatch_protocol::app::DownloadSummary;

use super::{Group, View, sort};
use crate::text;

/// One line of the list.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Line {
    Heading(Group, usize),
    /// The day the rows under it finished on.
    Day(NaiveDate),
    /// A download by its index in the library, with its place among what waits in its queue, and whether
    /// a day heading over it already says its day.
    Row(usize, Option<usize>, bool),
}

/// Everything the lines depend on.
#[derive(PartialEq, Eq)]
pub(super) struct Asked {
    pub revision: u64,
    pub view: View,
    pub needle: String,
    pub sort: sort::Sort,
    pub folded: [bool; Group::ALL.len()],
}

impl Asked {
    pub fn folded(folded: &HashSet<Group>) -> [bool; Group::ALL.len()] {
        Group::ALL.map(|group| folded.contains(&group))
    }

    fn is_folded(&self, group: Group) -> bool {
        self.folded[group.place()]
    }
}

/// What a day heading tells of the rows under it.
pub(super) struct Day {
    pub ids: Rc<[DownloadId]>,
    pub bytes: u64,
}

/// The list's lines, and what its headings and commands read from them.
pub(super) struct Laid {
    pub asked: Asked,
    pub lines: Vec<Line>,
    /// The rows shown, by their index in the library, in the list's order.
    pub rows: Vec<usize>,
    pub ids: Rc<[DownloadId]>,
    /// `rows` with each folded group's rows at its heading, in the list's order.
    with_folded: Vec<usize>,
    /// Each group's downloads the search finds, folded or not.
    groups: [Rc<[DownloadId]>; Group::ALL.len()],
    /// The day headings, by their line.
    days: HashMap<usize, Day>,
}

impl Laid {
    /// The view's downloads that hold the search in their name or link. A queue's view lists its
    /// line first, what waits numbered in the order it runs, then what is over by group; All downloads groups
    /// everything, each group under its heading, and a status's own page lists its downloads with
    /// no heading. Finished downloads in queue order go under the day they finished instead. A
    /// search shows everything it finds, folded or not.
    pub fn of(asked: Asked, downloads: &[DownloadSummary], frozen: &mut sort::Frozen) -> Self {
        let in_queue = matches!(asked.view, View::Queue(_));
        let mut lines = Vec::new();
        let mut members: [Vec<usize>; Group::ALL.len()] = Default::default();
        let mut place = 0;
        for (index, row) in downloads.iter().enumerate() {
            if !asked.view.shows(row) {
                continue;
            }
            let group = Group::of(row.status);
            let hit = found(row, &asked.needle);
            if in_queue && !group.is_over() {
                let waits = group == Group::UpNext;
                place += usize::from(waits);
                if hit {
                    lines.push(Line::Row(index, waits.then_some(place), false));
                }
            } else if hit {
                members[group.place()].push(index);
            }
        }
        let mut days = HashMap::new();
        for group in Group::ALL {
            let members = &mut members[group.place()];
            asked.sort.apply(group, members, downloads, frozen);
            if members.is_empty() {
                continue;
            }
            if group == Group::Finished && asked.sort.key == sort::Key::QueueOrder {
                finished_lines(members, downloads, &mut lines, &mut days);
                continue;
            }
            if !matches!(asked.view, View::Group(_)) {
                lines.push(Line::Heading(group, members.len()));
                if asked.needle.is_empty() && asked.is_folded(group) {
                    continue;
                }
            }
            lines.extend(members.iter().map(|index| Line::Row(*index, None, false)));
        }
        let rows: Vec<usize> = lines
            .iter()
            .filter_map(|line| match line {
                Line::Row(index, ..) => Some(*index),
                Line::Heading(..) | Line::Day(_) => None,
            })
            .collect();
        let ids = ids_of(&rows, downloads);
        let groups = Group::ALL.map(|group| ids_of(&members[group.place()], downloads));
        let mut with_folded = Vec::with_capacity(rows.len());
        for line in &lines {
            match line {
                Line::Row(index, ..) => with_folded.push(*index),
                Line::Heading(group, _) if asked.needle.is_empty() && asked.is_folded(*group) => {
                    with_folded.extend(&members[group.place()]);
                }
                Line::Heading(..) | Line::Day(_) => {}
            }
        }
        Self {
            asked,
            lines,
            rows,
            ids,
            with_folded,
            groups,
            days,
        }
    }

    /// The rows the selection's commands reach, in the list's order: those shown, and for a
    /// selection made `on_purpose` those of folded groups too (D160).
    pub fn under_commands(&self, on_purpose: bool) -> &[usize] {
        if on_purpose {
            &self.with_folded
        } else {
            &self.rows
        }
    }

    pub fn group_ids(&self, group: Group) -> Rc<[DownloadId]> {
        self.groups[group.place()].clone()
    }

    /// The heading `line` sits under: that line itself when it is one, else the last before it.
    pub fn heading_over(&self, line: usize) -> Option<usize> {
        self.lines
            .get(..=line)?
            .iter()
            .rposition(|line| matches!(line, Line::Heading(..) | Line::Day(_)))
    }

    /// The heading to pin over the list whose top is `top`: the one its top line sits under, once
    /// that heading's text, `text_from` down its own line, has begun to scroll away, so the pinned
    /// copy always covers it.
    pub fn pinned(&self, top: ListOffset, text_from: Pixels) -> Option<usize> {
        self.heading_over(top.item_ix)
            .filter(|heading| *heading < top.item_ix || top.offset_in_item >= text_from)
    }

    /// What the day heading on `line` tells.
    pub fn day(&self, line: usize) -> Option<&Day> {
        self.days.get(&line)
    }
}

/// The last lines, for as long as they hold.
#[derive(Default)]
pub(super) struct Kept(Option<Rc<Laid>>);

impl Kept {
    /// The lines for `asked`, laid out again only when it differs from the last.
    pub fn get(
        &mut self,
        asked: Asked,
        downloads: &[DownloadSummary],
        frozen: &mut sort::Frozen,
    ) -> Rc<Laid> {
        if let Some(laid) = self.0.as_ref().filter(|laid| laid.asked == asked) {
            return laid.clone();
        }
        let laid = Rc::new(Laid::of(asked, downloads, frozen));
        self.0 = Some(laid.clone());
        laid
    }
}

/// Puts finished `members` under the days they finished, with what each heading tells.
fn finished_lines(
    members: &[usize],
    downloads: &[DownloadSummary],
    lines: &mut Vec<Line>,
    days: &mut HashMap<usize, Day>,
) {
    let mut under: Vec<(usize, Vec<usize>)> = Vec::new();
    let mut last = None;
    for index in members {
        let day = finished_day(&downloads[*index]);
        if day != last {
            if let Some(day) = day {
                under.push((lines.len(), Vec::new()));
                lines.push(Line::Day(day));
            }
            last = day;
        }
        if let Some((_, rows)) = under.last_mut() {
            rows.push(*index);
        }
        lines.push(Line::Row(*index, None, day.is_some()));
    }
    days.extend(under.into_iter().map(|(line, rows)| {
        let bytes = rows
            .iter()
            .map(|index| &downloads[*index])
            .map(|row| row.total.unwrap_or(row.done))
            .sum();
        let ids = ids_of(&rows, downloads);
        (line, Day { ids, bytes })
    }));
}

fn ids_of(rows: &[usize], downloads: &[DownloadSummary]) -> Rc<[DownloadId]> {
    rows.iter().map(|index| downloads[*index].id).collect()
}

/// Whether a row holds the search in its name or link.
fn found(row: &DownloadSummary, needle: &str) -> bool {
    needle.is_empty()
        || row.file_name.to_lowercase().contains(needle)
        || row.url.to_lowercase().contains(needle)
}

/// The day a finished download goes under: the day it finished, or was added when that is unknown.
fn finished_day(row: &DownloadSummary) -> Option<NaiveDate> {
    text::local_day(row.completed_at.unwrap_or(row.added_at))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::px;
    use wirecatch_protocol::QueueId;
    use wirecatch_protocol::app::DownloadStatus;
    use wirecatch_test_support::downloads::download;

    fn row(id: i64, queue: i64, status: DownloadStatus) -> DownloadSummary {
        DownloadSummary {
            queue: QueueId(queue),
            status,
            ..download(id)
        }
    }

    fn asked(view: View, needle: &str, folded: &[Group]) -> Asked {
        Asked {
            revision: 0,
            view,
            needle: needle.to_owned(),
            sort: sort::Sort::default(),
            folded: Asked::folded(&folded.iter().copied().collect()),
        }
    }

    fn laid(asked: Asked, downloads: &[DownloadSummary]) -> Laid {
        Laid::of(asked, downloads, &mut sort::Frozen::default())
    }

    #[test]
    fn the_same_question_reuses_the_lines_and_each_part_of_it_replaces_them() {
        let downloads = [row(1, 1, DownloadStatus::Queued)];
        let mut frozen = sort::Frozen::default();
        let mut kept = Kept::default();
        let first = kept.get(asked(View::All, "", &[]), &downloads, &mut frozen);
        let again = kept.get(asked(View::All, "", &[]), &downloads, &mut frozen);
        assert!(Rc::ptr_eq(&first, &again));
        let others = [
            Asked {
                revision: 1,
                ..asked(View::All, "", &[])
            },
            asked(View::Group(Group::UpNext), "", &[]),
            asked(View::All, "file", &[]),
            Asked {
                sort: sort::Sort {
                    key: sort::Key::Name,
                    descending: false,
                },
                ..asked(View::All, "", &[])
            },
            asked(View::All, "", &[Group::UpNext]),
        ];
        let mut last = first;
        for other in others {
            let next = kept.get(other, &downloads, &mut frozen);
            assert!(!Rc::ptr_eq(&last, &next));
            last = next;
        }
    }

    #[test]
    fn all_downloads_groups_under_headings_and_a_fold_hides_rows_but_not_their_ids() {
        let downloads = [
            row(1, 1, DownloadStatus::Queued),
            row(2, 1, DownloadStatus::Downloading),
            row(3, 1, DownloadStatus::Queued),
        ];
        let shown = laid(asked(View::All, "", &[Group::UpNext]), &downloads);
        assert!(
            shown.lines
                == [
                    Line::Heading(Group::Downloading, 1),
                    Line::Row(1, None, false),
                    Line::Heading(Group::UpNext, 2),
                ]
        );
        assert_eq!(shown.rows, [1]);
        assert_eq!(*shown.ids, [DownloadId(2)]);
        assert_eq!(
            *shown.group_ids(Group::UpNext),
            [DownloadId(1), DownloadId(3)]
        );
    }

    // A tick on a folded heading is on purpose and its circle shows it, so commands reach those rows. A row only clicked, then folded away, stays out of reach of Delete.
    #[test]
    fn commands_reach_folded_rows_only_for_a_selection_made_on_purpose() {
        let downloads = [
            row(1, 1, DownloadStatus::Queued),
            row(2, 1, DownloadStatus::Downloading),
            row(3, 1, DownloadStatus::Queued),
        ];
        let folded = laid(asked(View::All, "", &[Group::UpNext]), &downloads);
        assert_eq!(folded.under_commands(true), [1, 0, 2]);
        assert_eq!(folded.under_commands(false), [1]);
        let open = laid(asked(View::All, "", &[]), &downloads);
        assert_eq!(open.under_commands(true), open.under_commands(false));
    }

    #[test]
    fn a_search_shows_what_it_finds_in_a_folded_group() {
        let downloads = [
            row(1, 1, DownloadStatus::Queued),
            row(2, 1, DownloadStatus::Queued),
        ];
        let shown = laid(asked(View::All, "file-2", &[Group::UpNext]), &downloads);
        assert!(shown.lines == [Line::Heading(Group::UpNext, 1), Line::Row(1, None, false)]);
    }

    #[test]
    fn a_queue_numbers_its_line_counting_rows_the_search_hides() {
        let downloads = [
            row(1, 1, DownloadStatus::Queued),
            row(2, 2, DownloadStatus::Queued),
            row(3, 1, DownloadStatus::Queued),
            row(4, 1, DownloadStatus::Cancelled),
        ];
        let shown = laid(asked(View::Queue(QueueId(1)), "file-3", &[]), &downloads);
        assert!(shown.lines == [Line::Row(2, Some(2), false)]);
        let whole = laid(asked(View::Queue(QueueId(1)), "", &[]), &downloads);
        assert_eq!(whole.rows, [0, 2, 3]);
    }

    #[test]
    fn only_what_waits_in_a_queue_has_a_number() {
        let downloads = [
            row(1, 1, DownloadStatus::Failed),
            row(2, 1, DownloadStatus::Downloading),
            row(3, 1, DownloadStatus::Queued),
            row(4, 1, DownloadStatus::Paused),
            row(5, 1, DownloadStatus::Queued),
        ];
        let shown = laid(asked(View::Queue(QueueId(1)), "", &[]), &downloads);
        assert!(
            shown.lines
                == [
                    Line::Row(0, None, false),
                    Line::Row(1, None, false),
                    Line::Row(2, Some(1), false),
                    Line::Row(3, None, false),
                    Line::Row(4, Some(2), false),
                ]
        );
    }

    #[test]
    fn a_line_sits_under_the_last_heading_before_it() {
        let downloads = [
            row(1, 1, DownloadStatus::Downloading),
            row(2, 1, DownloadStatus::Paused),
            row(3, 1, DownloadStatus::Paused),
        ];
        let shown = laid(asked(View::All, "", &[]), &downloads);
        assert_eq!(shown.heading_over(0), Some(0));
        assert_eq!(shown.heading_over(1), Some(0));
        assert_eq!(shown.heading_over(2), Some(2));
        assert_eq!(shown.heading_over(4), Some(2));
        assert_eq!(shown.heading_over(5), None);
        let page = laid(asked(View::Group(Group::Paused), "", &[]), &downloads);
        assert_eq!(page.heading_over(1), None);
    }

    #[test]
    fn a_heading_is_pinned_only_once_its_text_scrolls_away() {
        let downloads = [
            row(1, 1, DownloadStatus::Downloading),
            row(2, 1, DownloadStatus::Downloading),
        ];
        let shown = laid(asked(View::All, "", &[]), &downloads);
        let at = |item_ix, offset| ListOffset {
            item_ix,
            offset_in_item: px(offset),
        };
        let text_from = px(12.);
        assert_eq!(shown.pinned(at(0, 0.), text_from), None);
        assert_eq!(shown.pinned(at(0, 11.), text_from), None);
        assert_eq!(shown.pinned(at(0, 12.), text_from), Some(0));
        assert_eq!(shown.pinned(at(1, 0.), text_from), Some(0));
    }

    #[test]
    fn a_status_page_has_no_heading() {
        let downloads = [
            row(1, 1, DownloadStatus::Paused),
            row(2, 1, DownloadStatus::Queued),
        ];
        let shown = laid(asked(View::Group(Group::Paused), "", &[]), &downloads);
        assert!(shown.lines == [Line::Row(0, None, false)]);
    }

    #[test]
    fn a_day_heading_tells_the_rows_under_it() {
        let finished = |id, total, done| DownloadSummary {
            total,
            done,
            completed_at: download(id).added_at.into(),
            ..row(id, 1, DownloadStatus::Completed)
        };
        let downloads = [finished(1, Some(100), 100), finished(2, None, 50)];
        let shown = laid(asked(View::All, "", &[]), &downloads);
        assert!(matches!(shown.lines[0], Line::Day(_)));
        let day = shown.day(0).expect("the first line is a day");
        assert_eq!(day.bytes, 150);
        assert_eq!(day.ids.len(), 2);
    }
}
