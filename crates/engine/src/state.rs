//! The resume file kept next to a part file.

use std::fs::File;
use std::io::{self, Read, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::error::{IoAction, IoContext, Result};
use crate::http::Validators;
use crate::segment::ByteRange;

const VERSION: u32 = 1;

/// The largest resume file read: 16 MiB. A resume file lists the ranges still
/// to fetch, at most a few per connection (some hundred bytes); anything near
/// this size was not written by the engine, and reading it whole could take a
/// lot of memory.
const MAX_STATE_BYTES: u64 = 16 * crate::units::MIB;

/// Why a resume file could not be used. The download then starts over.
#[derive(Debug, thiserror::Error)]
pub(crate) enum LoadError {
    /// The file could not be read; [`io::ErrorKind::NotFound`] when there is none.
    #[error("it cannot be read")]
    Read(#[source] io::Error),
    /// The file is larger than any resume file the engine writes.
    #[error("it is larger than {MAX_STATE_BYTES} bytes")]
    TooLarge,
    /// The file is not a resume file, or is damaged.
    #[error("it is damaged")]
    Parse(#[source] serde_json::Error),
    /// The file was written by a version of the engine that keeps another format.
    #[error("it has format {0}, which this version does not read")]
    Version(u32),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumeState {
    pub version: u32,
    /// The URL the user asked for (before redirects). Used to match a rerun.
    pub url: String,
    pub total: Option<u64>,
    /// Whether the server accepted byte ranges. Without them nothing can resume.
    pub ranged: bool,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    /// Byte ranges not yet on disk.
    pub remaining: Vec<ByteRange>,
}

impl ResumeState {
    pub fn new(
        url: &str,
        total: Option<u64>,
        ranged: bool,
        validators: &Validators,
        remaining: Vec<ByteRange>,
    ) -> Self {
        Self {
            version: VERSION,
            url: url.to_string(),
            total,
            ranged,
            etag: validators.etag.clone(),
            last_modified: validators.last_modified.clone(),
            remaining,
        }
    }

    /// Reads the resume file at `path`.
    pub fn load(path: &Path) -> Result<Self, LoadError> {
        let file = File::open(path).map_err(LoadError::Read)?;
        let mut raw = Vec::new();
        file.take(MAX_STATE_BYTES + 1)
            .read_to_end(&mut raw)
            .map_err(LoadError::Read)?;
        if raw.len() as u64 > MAX_STATE_BYTES {
            return Err(LoadError::TooLarge);
        }
        let state: Self = serde_json::from_slice(&raw).map_err(LoadError::Parse)?;
        if state.version != VERSION {
            return Err(LoadError::Version(state.version));
        }
        Ok(state)
    }

    /// Writes to a temporary file first and flushes it to the disk before it
    /// replaces the old one, so neither a crash nor a power cut leaves a torn
    /// or empty resume file: at worst the previous one stays, which lists more
    /// to fetch, never less.
    pub fn save(&self, path: &Path) -> Result<()> {
        let mut tmp = path.as_os_str().to_owned();
        tmp.push(".tmp");
        let write = || -> io::Result<()> {
            let json = serde_json::to_vec(self).map_err(io::Error::other)?;
            {
                let mut file = File::create(&tmp)?;
                file.write_all(&json)?;
                file.sync_data()?;
            }
            std::fs::rename(&tmp, path)
        };
        write().context(IoAction::SaveResume, path)
    }

    /// Whether this state can continue a download the server now describes as
    /// `ranged`, `total` and `current`. Any change in size or validators means
    /// the file on the server changed and the old bytes are useless; ranges
    /// that cannot belong to the file mean the resume file is damaged.
    pub fn can_resume(&self, ranged: bool, total: Option<u64>, current: &Validators) -> bool {
        let same_file = self.same_file(ranged, total, current);
        if same_file && !total.is_some_and(|total| self.ranges_fit(total)) {
            warn!("the resume file lists ranges that cannot belong to the file; starting over");
            return false;
        }
        same_file
    }

    /// Whether the saved ranges can belong to a file of `total` bytes: each one
    /// non-empty and inside the file, in order, none overlapping the next. The
    /// engine writes no other kind, so any other came from outside it (a hand
    /// edit, a torn write) and is not to be trusted.
    fn ranges_fit(&self, total: u64) -> bool {
        let mut floor = 0;
        self.remaining.iter().all(|range| {
            let fits = floor <= range.start && range.start < range.end && range.end <= total;
            floor = range.end;
            fits
        })
    }

    /// Whether the server describes the same file this state was saved for.
    fn same_file(&self, ranged: bool, total: Option<u64>, current: &Validators) -> bool {
        let same = |saved: &Option<String>, now: &Option<String>| match (saved, now) {
            (Some(a), Some(b)) => a == b,
            (None, None) => true,
            // A validator that appeared or vanished counts as a change.
            _ => false,
        };
        self.ranged
            && ranged
            && total.is_some()
            && self.total == total
            && same(&self.etag, &current.etag)
            && same(&self.last_modified, &current.last_modified)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn validators(etag: Option<&str>) -> Validators {
        Validators {
            etag: etag.map(str::to_string),
            last_modified: None,
        }
    }

    #[test]
    fn round_trip_and_resume_rules() {
        let dir = wirecatch_test_support::private_dir("state").unwrap();
        let path = dir.path().join("f.bin.wcstate");
        let v = validators(Some("\"abc\""));
        let remaining = vec![ByteRange {
            start: 10,
            end: 100,
        }];
        ResumeState::new("https://x/f", Some(100), true, &v, remaining.clone())
            .save(&path)
            .unwrap();

        let loaded = ResumeState::load(&path).unwrap();
        assert_eq!(loaded.remaining, remaining);
        assert!(loaded.can_resume(true, Some(100), &v));
        assert!(!loaded.can_resume(false, Some(100), &v));
        assert!(!loaded.can_resume(true, Some(101), &v));
        assert!(!loaded.can_resume(true, Some(100), &validators(Some("\"def\""))));
        assert!(!loaded.can_resume(true, Some(100), &validators(None)));
        dir.remove().unwrap();
    }

    #[test]
    fn ranges_that_cannot_belong_to_the_file_are_not_resumed() {
        let v = validators(None);
        let state = |ranges: &[(u64, u64)]| {
            let remaining = ranges
                .iter()
                .map(|&(start, end)| ByteRange { start, end })
                .collect();
            ResumeState::new("https://x/f", Some(100), true, &v, remaining)
        };
        for fits in [&[][..], &[(0, 100)], &[(0, 10), (10, 20), (90, 100)]] {
            assert!(state(fits).can_resume(true, Some(100), &v), "{fits:?}");
        }
        for damaged in [
            &[(0, 101)][..],
            &[(5, 5)],
            &[(9, 5)],
            &[(0, 50), (40, 60)],
            &[(50, 60), (0, 10)],
            &[(u64::MAX - 1, u64::MAX)],
        ] {
            assert!(
                !state(damaged).can_resume(true, Some(100), &v),
                "{damaged:?}"
            );
        }
    }

    /// A download of unknown size cannot resume; its resume file is set aside
    /// without a warning that its ranges are damaged.
    #[test]
    fn an_unknown_size_is_not_taken_for_damaged_ranges() {
        use std::io::Write;
        use std::sync::{Arc, Mutex};

        #[derive(Clone, Default)]
        struct Captured(Arc<Mutex<Vec<u8>>>);
        impl Write for Captured {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(buf);
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let captured = Captured::default();
        let writer = captured.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .finish();
        let v = validators(None);
        let unknown = ResumeState::new("https://x/f", None, true, &v, Vec::new());
        let resumes =
            tracing::subscriber::with_default(subscriber, || unknown.can_resume(true, None, &v));
        assert!(!resumes);
        let logged = || String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
        assert!(logged().is_empty(), "{}", logged());

        // The capture works: damaged ranges of a known size are logged.
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_writer({
                let writer = captured.clone();
                move || writer.clone()
            })
            .finish();
        let damaged = ResumeState::new(
            "https://x/f",
            Some(10),
            true,
            &v,
            vec![ByteRange { start: 5, end: 50 }],
        );
        tracing::subscriber::with_default(subscriber, || damaged.can_resume(true, Some(10), &v));
        assert!(
            logged().contains("cannot belong to the file"),
            "{}",
            logged()
        );
    }

    #[test]
    fn an_oversized_resume_file_is_not_read() {
        let dir = wirecatch_test_support::private_dir("state-size").unwrap();
        let path = dir.path().join("f.bin.wcstate");
        // Sparse: no 16 MiB is written.
        File::create(&path)
            .unwrap()
            .set_len(MAX_STATE_BYTES + 1)
            .unwrap();
        assert!(matches!(ResumeState::load(&path), Err(LoadError::TooLarge)));
        dir.remove().unwrap();
    }
}
