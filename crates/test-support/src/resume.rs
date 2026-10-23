//! What a resumed download asks for, against what its earlier run kept.
//!
//! A test that bounds the bytes a server wrote while a download resumes counts what socket buffers took in before a connection was broken off, which grows with the machine's load. These compare the ranges the resumed run asked for with the ranges the engine's resume file says are already on disk, which no load changes.

use std::path::Path;

use crate::TestResult;
use crate::http::Seen;

/// The ranges `[start, end)` of a file of `len` bytes that the resume file at `state` says are on disk: everything but its `remaining`.
///
/// # Errors
///
/// The file cannot be read, or is not a resume file with its `remaining` ranges.
pub fn kept_ranges(state: &Path, len: u64) -> TestResult<Vec<(u64, u64)>> {
    let state: serde_json::Value = serde_json::from_slice(&std::fs::read(state)?)?;
    let remaining = state["remaining"]
        .as_array()
        .ok_or("the resume file lists no remaining ranges")?;
    let mut kept = Vec::new();
    let mut from = 0;
    for range in remaining {
        let bound = |key: &str| {
            range[key]
                .as_u64()
                .ok_or("a remaining range without its bounds")
        };
        let (start, end) = (bound("start")?, bound("end")?);
        if from < start {
            kept.push((from, start));
        }
        from = end;
    }
    if from < len {
        kept.push((from, len));
    }
    Ok(kept)
}

/// The ranges among `asked`, in a file of `len` bytes, that overlap a range in `kept`: bytes asked for again. A request without a `Range` header asks for the whole file.
#[must_use]
pub fn asked_again(asked: &[Seen], kept: &[(u64, u64)], len: u64) -> Vec<(u64, u64)> {
    asked
        .iter()
        .map(|seen| {
            seen.range()
                .map_or((0, len), |(start, end)| (start, end.unwrap_or(len)))
        })
        .filter(|&(start, end)| kept.iter().any(|&(from, to)| start < to && from < end))
        .collect()
}
