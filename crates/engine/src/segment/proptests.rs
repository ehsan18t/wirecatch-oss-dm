//! Property tests for the segment table: random sequences of acquire, claim,
//! flush and release, driven the way workers drive the table, must keep the
//! table consistent after every step and still finish the whole file.

use proptest::prelude::*;

use super::*;

const WORKERS: usize = 4;

#[derive(Debug, Clone)]
enum Op {
    /// An idle worker asks for work.
    Acquire(usize),
    /// A worker receives this many bytes.
    Claim(usize, u64),
    /// A worker's pending write completes, covering this percentage of what it
    /// has claimed but not yet written.
    Flush(usize, u8),
    /// A worker gives its segment back (error, cancel or stepping back).
    Release(usize),
}

fn ops(max_claim: u64) -> impl Strategy<Value = Vec<Op>> {
    let op = prop_oneof![
        3 => (0..WORKERS).prop_map(Op::Acquire),
        4 => (0..WORKERS, 1..=max_claim).prop_map(|(w, n)| Op::Claim(w, n)),
        2 => (0..WORKERS, 0u8..=100).prop_map(|(w, p)| Op::Flush(w, p)),
        1 => (0..WORKERS).prop_map(Op::Release),
    ];
    prop::collection::vec(op, 0..200)
}

/// The table plus what the test knows independently of it.
struct Model {
    table: SegmentTable,
    /// The byte ranges the table must cover: the whole file for a fresh
    /// download, the saved ranges for a resumed one.
    file: Vec<ByteRange>,
    /// Where each segment started; segment indexes never change.
    starts: Vec<u64>,
    /// The segment each worker is working on.
    current: [Option<usize>; WORKERS],
    /// Bytes written so far, counted from the flush calls alone.
    written: u64,
}

impl Model {
    fn new(table: SegmentTable, file: Vec<ByteRange>) -> Self {
        let starts = table.segments.iter().map(|s| s.flushed).collect();
        Self {
            table,
            file,
            starts,
            current: [None; WORKERS],
            written: 0,
        }
    }

    fn file_bytes(&self) -> u64 {
        self.file.iter().map(|r| r.end - r.start).sum()
    }

    fn apply(&mut self, op: &Op) {
        match *op {
            Op::Acquire(w) => {
                if self.current[w].is_some() {
                    return;
                }
                let Some(a) = self.table.acquire(w) else {
                    return;
                };
                assert!(a.from < a.to, "empty assignment {a:?}");
                if a.index == self.starts.len() {
                    self.starts.push(a.from);
                }
                let seg = &self.table.segments[a.index];
                assert_eq!((seg.flushed, seg.claimed, seg.end), (a.from, a.from, a.to));
                assert!(
                    !self.current.contains(&Some(a.index)),
                    "segment {} handed to two workers",
                    a.index
                );
                self.current[w] = Some(a.index);
            }
            Op::Claim(w, len) => {
                let Some(i) = self.current[w] else { return };
                let before = self.table.segments[i].claimed;
                let (allowed, done) = self.table.claim(i, len);
                let seg = &self.table.segments[i];
                assert!(allowed <= len);
                assert_eq!(seg.claimed, before + allowed);
                assert_eq!(done, seg.claimed == seg.end);
                if done {
                    // A finished stretch writes out everything before the worker
                    // asks for more; it stays the owner of the finished segment.
                    self.flush_to(i, seg.claimed);
                    self.current[w] = None;
                }
            }
            Op::Flush(w, percent) => {
                let Some(i) = self.current[w] else { return };
                let seg = &self.table.segments[i];
                let upto = seg.flushed + (seg.claimed - seg.flushed) * u64::from(percent) / 100;
                self.flush_to(i, upto);
            }
            Op::Release(w) => {
                let Some(i) = self.current[w].take() else {
                    return;
                };
                self.table.release(i);
            }
        }
    }

    fn flush_to(&mut self, index: usize, upto: u64) {
        let before = self.table.segments[index].flushed;
        self.table.mark_flushed(index, upto);
        let after = self.table.segments[index].flushed;
        assert!(after >= before, "flushed offset went backwards");
        self.written += after - before;
    }

    fn check(&self) {
        let segs = &self.table.segments;
        assert_eq!(segs.len(), self.starts.len());

        // Offsets are ordered inside every segment.
        for (i, s) in segs.iter().enumerate() {
            assert!(
                self.starts[i] <= s.flushed && s.flushed <= s.claimed && s.claimed <= s.end,
                "segment {i}: start {} flushed {} claimed {} end {}",
                self.starts[i],
                s.flushed,
                s.claimed,
                s.end
            );
        }

        // Every busy worker owns its segment.
        for (w, current) in self.current.iter().enumerate() {
            if let Some(i) = current {
                assert_eq!(segs[*i].owner, Some(w));
            }
        }

        // Segments never overlap and together cover exactly the file: each lies
        // inside one file range, they are disjoint, and their lengths add up.
        let mut spans: Vec<(u64, u64)> = segs
            .iter()
            .enumerate()
            .map(|(i, s)| (self.starts[i], s.end))
            .collect();
        spans.sort_unstable();
        for (start, end) in &spans {
            assert!(start < end, "empty segment {start}..{end}");
            assert!(
                self.file.iter().any(|r| r.start <= *start && *end <= r.end),
                "segment {start}..{end} is outside the file"
            );
        }
        for pair in spans.windows(2) {
            assert!(pair[0].1 <= pair[1].0, "segments overlap: {pair:?}");
        }
        let covered: u64 = spans.iter().map(|(a, b)| b - a).sum();
        assert_eq!(covered, self.file_bytes(), "segments do not cover the file");

        // Written bytes as the table sees them match what was flushed.
        let flushed: u64 = segs
            .iter()
            .enumerate()
            .map(|(i, s)| s.flushed - self.starts[i])
            .sum();
        assert_eq!(flushed, self.written);

        // What the resume file would store: sorted, disjoint, and together with
        // the written bytes exactly the file.
        let remaining = self.table.remaining();
        for pair in remaining.windows(2) {
            assert!(pair[0].end <= pair[1].start, "remaining overlaps: {pair:?}");
        }
        let remaining_sum: u64 = remaining.iter().map(|r| r.end - r.start).sum();
        assert_eq!(remaining_sum, self.table.remaining_bytes());
        assert_eq!(
            self.table.remaining_bytes() + self.written,
            self.file_bytes()
        );
        assert_eq!(self.table.is_complete(), self.table.remaining_bytes() == 0);
    }

    /// Lets every worker run to the end with no more failures; the file must
    /// then be complete with every byte written exactly once.
    fn drain(&mut self) {
        loop {
            let mut busy = false;
            for w in 0..WORKERS {
                self.apply(&Op::Acquire(w));
                if self.current[w].is_some() {
                    busy = true;
                    self.apply(&Op::Claim(w, u64::MAX / 2));
                    self.check();
                }
            }
            if !busy {
                break;
            }
        }
        assert!(
            self.table.is_complete(),
            "work left over: {:?}",
            self.table.remaining()
        );
        assert_eq!(self.written, self.file_bytes());
    }
}

/// Random disjoint ranges inside `0..len`: the parts a resumed download still needs.
fn saved_ranges() -> impl Strategy<Value = (u64, Vec<ByteRange>)> {
    (1u64..=4_000_000, prop::collection::vec(any::<u64>(), 0..10)).prop_map(|(len, cuts)| {
        let mut points: Vec<u64> = cuts.into_iter().map(|c| c % (len + 1)).collect();
        points.push(0);
        points.push(len);
        points.sort_unstable();
        points.dedup();
        let ranges = points
            .windows(2)
            .step_by(2)
            .map(|w| ByteRange {
                start: w[0],
                end: w[1],
            })
            .collect();
        (len, ranges)
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn fresh_table_stays_consistent(
        len in 1u64..=8_000_000,
        min_split in 1u64..=1_000_000,
        ops in ops(2_000_000),
    ) {
        let mut model = Model::new(
            SegmentTable::new(Some(len), min_split),
            vec![ByteRange { start: 0, end: len }],
        );
        model.check();
        for op in &ops {
            model.apply(op);
            model.check();
        }
        model.drain();
        prop_assert_eq!(model.table.file_len(), len);
    }

    #[test]
    fn resumed_table_stays_consistent(
        (_len, saved) in saved_ranges(),
        min_split in 1u64..=500_000,
        ops in ops(1_000_000),
    ) {
        let mut model = Model::new(SegmentTable::from_remaining(&saved, min_split), saved.clone());
        model.check();
        for op in &ops {
            model.apply(op);
            model.check();
        }
        model.drain();
    }
}
