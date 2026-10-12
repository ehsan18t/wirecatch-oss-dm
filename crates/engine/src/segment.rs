//! The table of byte ranges still to download, and the dynamic splitting rule.
//!
//! Each segment tracks three offsets: `claimed` (bytes a worker has taken off the
//! wire for it), `flushed` (bytes known to be written to the file) and `end`.
//! Splitting only ever cuts above `claimed`, so a worker never loses bytes it
//! already received, and resume always restarts from `flushed`.

use std::sync::{Mutex, MutexGuard, PoisonError};

use serde::{Deserialize, Serialize};

/// Marks a segment whose length is unknown (server sent no size).
pub const UNKNOWN_END: u64 = u64::MAX;

/// A half-open byte range `[start, end)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ByteRange {
    pub start: u64,
    pub end: u64,
}

#[derive(Debug)]
struct Segment {
    claimed: u64,
    flushed: u64,
    end: u64,
    owner: Option<usize>,
}

impl Segment {
    fn unclaimed(&self) -> u64 {
        self.end.saturating_sub(self.claimed)
    }
}

/// Work handed to a worker: download `[from, to)` into segment `index`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Assignment {
    pub index: usize,
    pub from: u64,
    pub to: u64,
}

#[derive(Debug)]
pub struct SegmentTable {
    segments: Vec<Segment>,
    min_split: u64,
    splittable: bool,
}

impl SegmentTable {
    /// One segment covering the whole file. `total == None` means unknown length,
    /// which also disables splitting.
    pub fn new(total: Option<u64>, min_split: u64) -> Self {
        let end = total.unwrap_or(UNKNOWN_END);
        Self {
            segments: vec![Segment {
                claimed: 0,
                flushed: 0,
                end,
                owner: None,
            }],
            min_split: min_split.max(1),
            splittable: total.is_some(),
        }
    }

    /// Rebuilds the table from the ranges a previous run still had to fetch.
    pub fn from_remaining(remaining: &[ByteRange], min_split: u64) -> Self {
        let segments = remaining
            .iter()
            .filter(|r| r.start < r.end)
            .map(|r| Segment {
                claimed: r.start,
                flushed: r.start,
                end: r.end,
                owner: None,
            })
            .collect();
        Self {
            segments,
            min_split: min_split.max(1),
            splittable: true,
        }
    }

    /// Turns off splitting, for servers that do not accept byte ranges.
    pub fn without_splitting(mut self) -> Self {
        self.splittable = false;
        self
    }

    /// Gives worker `worker` something to do: first any segment nobody owns,
    /// otherwise the upper half of the segment with the most unclaimed bytes.
    pub fn acquire(&mut self, worker: usize) -> Option<Assignment> {
        if let Some((index, seg)) = self
            .segments
            .iter_mut()
            .enumerate()
            .find(|(_, s)| s.owner.is_none() && s.unclaimed() > 0)
        {
            seg.owner = Some(worker);
            seg.claimed = seg.flushed;
            return Some(Assignment {
                index,
                from: seg.claimed,
                to: seg.end,
            });
        }

        if !self.splittable {
            return None;
        }
        let (victim, remaining) = self
            .segments
            .iter()
            .enumerate()
            .map(|(i, s)| (i, s.unclaimed()))
            .max_by_key(|&(_, rem)| rem)?;
        if remaining < 2 * self.min_split {
            return None;
        }

        let seg = &mut self.segments[victim];
        let mid = align_down(seg.claimed + remaining / 2, seg.claimed);
        let old_end = seg.end;
        seg.end = mid;
        self.segments.push(Segment {
            claimed: mid,
            flushed: mid,
            end: old_end,
            owner: Some(worker),
        });
        Some(Assignment {
            index: self.segments.len() - 1,
            from: mid,
            to: old_end,
        })
    }

    /// Records `len` freshly received bytes for a segment. Returns how many of them
    /// belong to it (the end may have moved down because of a split) and whether
    /// the segment is now fully claimed.
    pub fn claim(&mut self, index: usize, len: u64) -> (u64, bool) {
        let seg = &mut self.segments[index];
        let allowed = len.min(seg.unclaimed());
        seg.claimed += allowed;
        (allowed, seg.claimed >= seg.end)
    }

    /// Records that `written` is on disk for this segment. Only a write that continues from the flushed offset counts: one that starts above it would leave a hole (an earlier write that failed) that resume must never treat as downloaded, so it is ignored and that part is fetched again.
    pub fn mark_flushed(&mut self, index: usize, written: ByteRange) {
        let seg = &mut self.segments[index];
        debug_assert!(written.end <= seg.claimed);
        if written.start <= seg.flushed {
            seg.flushed = seg.flushed.max(written.end);
        }
    }

    /// For an unknown-length download: the stream ended, so this is the real end.
    pub fn close_unknown(&mut self, index: usize) {
        let seg = &mut self.segments[index];
        seg.end = seg.flushed;
        seg.claimed = seg.flushed;
    }

    /// Hands a segment back. Bytes claimed but never flushed are forgotten so the
    /// next owner fetches them again.
    pub fn release(&mut self, index: usize) {
        let seg = &mut self.segments[index];
        seg.claimed = seg.flushed;
        seg.owner = None;
    }

    /// Whether some segment other than `index` has an owner and bytes still to
    /// claim. Its worker will ask for more work when it finishes, so anything
    /// handed back now is not left without a worker to take it.
    pub fn others_active(&self, index: usize) -> bool {
        self.segments
            .iter()
            .enumerate()
            .any(|(i, s)| i != index && s.owner.is_some() && s.unclaimed() > 0)
    }

    /// `(claimed, end)` of a segment.
    pub fn position(&self, index: usize) -> (u64, u64) {
        let seg = &self.segments[index];
        (seg.claimed, seg.end)
    }

    /// Length of the file this table describes (valid once complete).
    pub fn file_len(&self) -> u64 {
        self.segments.iter().map(|s| s.end).max().unwrap_or(0)
    }

    pub fn is_complete(&self) -> bool {
        self.segments.iter().all(|s| s.flushed >= s.end)
    }

    /// Ranges still missing on disk; this is what the resume file stores.
    pub fn remaining(&self) -> Vec<ByteRange> {
        let mut ranges: Vec<ByteRange> = self
            .segments
            .iter()
            .filter(|s| s.flushed < s.end)
            .map(|s| ByteRange {
                start: s.flushed,
                end: s.end,
            })
            .collect();
        ranges.sort_by_key(|r| r.start);
        ranges
    }

    pub fn remaining_bytes(&self) -> u64 {
        self.segments
            .iter()
            .map(|s| s.end.saturating_sub(s.flushed))
            .sum()
    }
}

/// A [`SegmentTable`] shared by the connections of one download.
#[derive(Debug)]
pub struct SharedTable(Mutex<SegmentTable>);

impl SharedTable {
    pub fn new(table: SegmentTable) -> Self {
        Self(Mutex::new(table))
    }

    /// Locks the table.
    ///
    /// A lock poisoned by a panic in another holder is taken over as it is.
    /// Nothing done under this lock is meant to panic, so poisoning means a bug
    /// that the panic itself has already reported. Wirecatch's release builds
    /// abort on panic (`panic = "abort"`), so there a poisoned lock cannot be
    /// seen at all; in debug and test builds, or a program that unwinds, the
    /// panicking connection's task ends with
    /// [`Error::Internal`](crate::Error::Internal). Refusing the lock as well
    /// would only add a second, less telling panic.
    pub fn lock(&self) -> MutexGuard<'_, SegmentTable> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The end (exclusive) of the next request for the range `[from, to)` when no
/// request may ask for more than `limit` bytes: `to` itself without a limit, and
/// for a range of unknown length, whose end is not known to stop at.
///
/// A connection asks for `[from, request_end)`, and once that has arrived asks
/// for the next piece from where it got to, until the segment is done. As the
/// segment's end can only move down (when another connection takes part of it),
/// the requests always stay within it, and their ends never pass the end the
/// segment had when each was sent.
pub fn request_end(from: u64, to: u64, limit: Option<u64>) -> u64 {
    match limit {
        Some(limit) if limit > 0 && to != UNKNOWN_END => to.min(from.saturating_add(limit)),
        _ => to,
    }
}

/// Rounds a split point down to a 64 KiB boundary so writes stay page aligned,
/// unless that would fall at or below `floor`.
fn align_down(at: u64, floor: u64) -> u64 {
    const ALIGN: u64 = 64 * 1024;
    let aligned = at - at % ALIGN;
    if aligned > floor { aligned } else { at }
}

#[cfg(test)]
mod proptests;

#[cfg(test)]
mod tests {
    use super::*;

    const MIB: u64 = 1024 * 1024;

    fn range(start: u64, end: u64) -> ByteRange {
        ByteRange { start, end }
    }

    #[test]
    fn a_write_above_the_flushed_offset_is_not_counted() {
        let mut t = SegmentTable::new(Some(4 * MIB), MIB);
        t.acquire(0).unwrap();
        t.claim(0, 3 * MIB);
        // The write of [0, 1 MiB) failed; the next one must not cover for it.
        t.mark_flushed(0, range(MIB, 2 * MIB));
        assert_eq!(t.remaining()[0].start, 0);
        t.mark_flushed(0, range(0, MIB));
        t.mark_flushed(0, range(MIB, 2 * MIB));
        assert_eq!(t.remaining()[0].start, 2 * MIB);
    }

    #[test]
    fn first_worker_gets_whole_file_then_others_split() {
        let mut t = SegmentTable::new(Some(64 * MIB), MIB);
        assert_eq!(
            t.acquire(0),
            Some(Assignment {
                index: 0,
                from: 0,
                to: 64 * MIB
            })
        );
        let a1 = t.acquire(1).unwrap();
        assert_eq!((a1.from, a1.to), (32 * MIB, 64 * MIB));
        assert_eq!(t.position(0).1, 32 * MIB);
        // Next split halves one of the two 32 MiB pieces.
        let a2 = t.acquire(2).unwrap();
        assert_eq!(a2.to - a2.from, 16 * MIB);
    }

    #[test]
    fn splitting_never_cuts_below_claimed_bytes() {
        let mut t = SegmentTable::new(Some(10 * MIB), MIB);
        t.acquire(0).unwrap();
        t.claim(0, 9 * MIB);
        // Only 1 MiB unclaimed, below 2 * min_split: no split.
        assert_eq!(t.acquire(1), None);
        let (allowed, done) = t.claim(0, 5 * MIB);
        assert_eq!((allowed, done), (MIB, true));
    }

    #[test]
    fn claim_respects_end_moved_by_split() {
        let mut t = SegmentTable::new(Some(8 * MIB), MIB);
        t.acquire(0).unwrap();
        t.acquire(1).unwrap(); // worker 0 now ends at 4 MiB
        let (allowed, done) = t.claim(0, 5 * MIB);
        assert_eq!(allowed, 4 * MIB);
        assert!(done);
    }

    #[test]
    fn released_segment_is_reassigned_from_flushed_offset() {
        let mut t = SegmentTable::new(Some(8 * MIB), MIB);
        t.acquire(0).unwrap();
        t.claim(0, 3 * MIB);
        t.mark_flushed(0, range(0, 2 * MIB));
        t.release(0);
        let a = t.acquire(5).unwrap();
        assert_eq!((a.index, a.from, a.to), (0, 2 * MIB, 8 * MIB));
    }

    #[test]
    fn remaining_and_completion() {
        let mut t = SegmentTable::new(Some(4 * MIB), MIB);
        t.acquire(0).unwrap();
        t.acquire(1).unwrap();
        t.claim(0, 2 * MIB);
        t.mark_flushed(0, range(0, 2 * MIB));
        assert_eq!(
            t.remaining(),
            vec![ByteRange {
                start: 2 * MIB,
                end: 4 * MIB
            }]
        );
        assert!(!t.is_complete());
        t.claim(1, 2 * MIB);
        t.mark_flushed(1, range(2 * MIB, 4 * MIB));
        assert!(t.is_complete());
        assert_eq!(t.remaining_bytes(), 0);
    }

    #[test]
    fn resume_table_hands_out_saved_ranges() {
        let saved = [
            ByteRange { start: 0, end: 10 },
            ByteRange { start: 50, end: 50 },
        ];
        let mut t = SegmentTable::from_remaining(&saved, 1);
        assert_eq!(
            t.acquire(0),
            Some(Assignment {
                index: 0,
                from: 0,
                to: 10
            })
        );
    }

    #[test]
    fn unknown_length_never_splits() {
        let mut t = SegmentTable::new(None, 1);
        t.acquire(0).unwrap();
        assert_eq!(t.acquire(1), None);
        t.claim(0, 500);
        t.mark_flushed(0, range(0, 500));
        t.close_unknown(0);
        assert!(t.is_complete());
    }

    #[test]
    fn others_active_ignores_finished_and_unowned_segments() {
        let mut t = SegmentTable::new(Some(8 * MIB), MIB);
        t.acquire(0).unwrap();
        assert!(!t.others_active(0));
        let a1 = t.acquire(1).unwrap();
        assert!(t.others_active(a1.index));
        // Segment 0 fully claimed: its worker is about to ask for more, but it
        // no longer counts as fetching.
        t.claim(0, 4 * MIB);
        assert!(!t.others_active(a1.index));
        t.release(a1.index);
        assert!(!t.others_active(0));
    }

    #[test]
    fn split_points_are_aligned() {
        let mut t = SegmentTable::new(Some(10 * MIB + 12345), MIB);
        t.acquire(0).unwrap();
        let a = t.acquire(1).unwrap();
        assert_eq!(a.from % (64 * 1024), 0);
    }
}
