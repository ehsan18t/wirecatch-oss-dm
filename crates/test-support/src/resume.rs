//! What a resumed download asks for, against what its earlier run kept.
//!
//! A test that bounds the bytes a server wrote while a download resumes counts what socket buffers took in before a connection was broken off, which grows with the machine's load. These compare the ranges the resumed run asked for with the ranges the engine's resume file says are already on disk, which no load changes.

use std::path::Path;

use crate::TestResult;
use crate::http::Seen;

/// What an earlier run of a download saved in its resume file.
#[derive(Debug, Clone, Copy)]
pub struct SavedRun<'a> {
    /// The link it was made for.
    pub url: &'a str,
    /// The file's size, if the server gave it.
    pub total: Option<u64>,
    /// Whether the server took byte ranges.
    pub ranged: bool,
    /// The `ETag` it saw.
    pub etag: Option<&'a str>,
    /// The `Last-Modified` it saw.
    pub last_modified: Option<&'a str>,
    /// The ranges still missing.
    pub remaining: &'a [(u64, u64)],
}

/// Writes `run` to the resume file at `state`, in the engine's format.
///
/// # Errors
///
/// The file cannot be written.
pub fn write_resume_file(state: &Path, run: &SavedRun<'_>) -> std::io::Result<()> {
    let remaining: Vec<_> = run
        .remaining
        .iter()
        .map(|&(start, end)| serde_json::json!({ "start": start, "end": end }))
        .collect();
    let saved = serde_json::json!({
        "version": 1,
        "url": run.url,
        "total": run.total,
        "ranged": run.ranged,
        "etag": run.etag,
        "last_modified": run.last_modified,
        "remaining": remaining,
    });
    std::fs::write(state, saved.to_string())
}

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
