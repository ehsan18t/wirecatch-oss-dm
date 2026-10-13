//! The `sidx` box of a file indexed by `SegmentBase`: its subsegments as byte ranges.

#![forbid(unsafe_code)]

use std::ops::Range;

use url::Url;

use crate::plan::{AddressBudget, Piece};

/// The bits of a `sidx` reference that hold its size; the top bit says whether it points to another index.
const SIDX_SIZE_MASK: u64 = 0x7FFF_FFFF;

/// Splits a file indexed by a `sidx` box into its init section (everything before the first media byte, the index included) and its subsegments. `data` holds the bytes of `index`. `None` when the index is not a plain single-level `sidx`; the caller then fetches the whole file.
pub(crate) fn sidx_pieces(url: &Url, index: &Range<u64>, data: &[u8]) -> Option<Vec<Piece>> {
    let u32_at = |at: usize| -> Option<u64> {
        Some(u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?).into())
    };
    let u64_at = |at: usize| -> Option<u64> {
        Some(u64::from_be_bytes(data.get(at..at + 8)?.try_into().ok()?))
    };
    if data.get(4..8)? != b"sidx" {
        return None;
    }
    let (box_size, mut at) = match u32_at(0)? {
        1 => (u64_at(8)?, 16),
        0 => (u64::try_from(data.len()).ok()?, 8),
        size => (size, 8),
    };
    if box_size < u64::try_from(at).ok()? {
        // A box smaller than its own header is not one, and could make the init section an empty range, which no request can ask for.
        return None;
    }
    let version = *data.get(at)?;
    at += 4 + 4 + 4; // version and flags, reference_ID, timescale
    let first_offset = if version == 0 {
        at += 4;
        let first = u32_at(at)?;
        at += 4;
        first
    } else {
        at += 8;
        let first = u64_at(at)?;
        at += 8;
        first
    };
    at += 2; // reserved
    let count = u16::from_be_bytes(data.get(at..at + 2)?.try_into().ok()?);
    at += 2;

    let mut offset = index
        .start
        .checked_add(box_size)?
        .checked_add(first_offset)?;
    let mut budget = AddressBudget::default();
    let mut init = Piece::plain(url.clone(), Some(0..offset));
    init.init = true;
    let mut pieces = vec![init];
    for _ in 0..count {
        // Over the budget, the whole file is fetched instead: one address.
        if budget.spend(url).is_err() {
            return None;
        }
        let reference = u32_at(at)?;
        at += 12;
        if reference & SIDX_SIZE_MASK != reference {
            // A reference to another index: not handled.
            return None;
        }
        let size = reference & SIDX_SIZE_MASK;
        if size == 0 {
            // An empty subsegment would be an empty range, which no request can ask for.
            return None;
        }
        let end = offset.checked_add(size)?;
        pieces.push(Piece::plain(url.clone(), Some(offset..end)));
        offset = end;
    }
    (pieces.len() > 1).then_some(pieces)
}
