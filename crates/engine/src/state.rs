//! The resume file kept next to a part file.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::http::Validators;
use crate::segment::ByteRange;

const VERSION: u32 = 1;

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

    pub fn load(path: &Path) -> Result<Self> {
        let raw = std::fs::read(path)?;
        let state: Self = serde_json::from_slice(&raw).map_err(|e| Error::State(e.to_string()))?;
        if state.version != VERSION {
            return Err(Error::State(format!(
                "unsupported version {}",
                state.version
            )));
        }
        Ok(state)
    }

    /// Writes to a temporary file first so a crash never leaves a torn state file.
    pub fn save(&self, path: &Path) -> Result<()> {
        let mut tmp = path.as_os_str().to_owned();
        tmp.push(".tmp");
        let json = serde_json::to_vec(self).map_err(|e| Error::State(e.to_string()))?;
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    /// Whether this state can continue a download the server now describes as
    /// `ranged`, `total` and `current`. Any change in size or validators means
    /// the file on the server changed and the old bytes are useless.
    pub fn can_resume(&self, ranged: bool, total: Option<u64>, current: &Validators) -> bool {
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
    }
}
