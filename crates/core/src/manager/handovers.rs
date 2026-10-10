//! Recent handovers from the extension, keyed by the extension's handover id, so a
//! repeated add gets the same answer and a withdrawal finds what it refers to.

use std::collections::{HashMap, VecDeque};

use wirecatch_protocol::DownloadId;
use wirecatch_protocol::extension::AddedDownload;

use crate::error::{CoreError, Result};

/// How many handovers are remembered. The extension retries within seconds, so the
/// oldest ones can be forgotten long before this fills up.
const MAX_REMEMBERED: usize = 1000;
/// Longest handover id accepted.
const MAX_ID_LEN: usize = 128;

/// What a handover turned into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Target {
    /// Waiting for the user's confirmation (a pending capture id).
    Pending(u64),
    /// A download in the list.
    Download(DownloadId),
    /// The user dismissed it; nothing was downloaded.
    Dismissed,
}

/// One remembered handover.
#[derive(Debug, Clone)]
pub(super) struct Handover {
    /// The answer given the first time, repeated for a duplicate add.
    pub reply: AddedDownload,
    pub target: Target,
}

/// A bounded memory of handovers; the oldest are forgotten first.
#[derive(Debug, Default)]
pub(super) struct Handovers {
    order: VecDeque<String>,
    known: HashMap<String, Handover>,
}

impl Handovers {
    pub fn get(&self, id: &str) -> Option<&Handover> {
        self.known.get(id)
    }

    /// Remembers a handover, forgetting the oldest one if full.
    pub fn insert(&mut self, id: &str, handover: Handover) {
        if self.known.insert(id.to_string(), handover).is_none() {
            self.order.push_back(id.to_string());
        }
        while self.order.len() > MAX_REMEMBERED {
            if let Some(oldest) = self.order.pop_front() {
                self.known.remove(&oldest);
            }
        }
    }

    /// Points a handover at what it became, keeping its first answer.
    pub fn retarget(&mut self, id: &str, target: Target) {
        if let Some(handover) = self.known.get_mut(id) {
            handover.target = target;
        }
    }

    /// Forgets a handover.
    pub fn remove(&mut self, id: &str) {
        if self.known.remove(id).is_some() {
            self.order.retain(|known| known != id);
        }
    }
}

/// Checks a handover id from the extension.
pub(super) fn check_id(id: &str) -> Result<()> {
    if id.is_empty() || id.len() > MAX_ID_LEN {
        return Err(CoreError::Invalid(format!(
            "The browser sent a download without a valid handover id (1 to {MAX_ID_LEN} characters). Update the extension."
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wirecatch_protocol::extension::AddAction;

    fn handover(n: i64) -> Handover {
        Handover {
            reply: AddedDownload {
                id: Some(DownloadId(n)),
                action: AddAction::Queued,
            },
            target: Target::Download(DownloadId(n)),
        }
    }

    #[test]
    fn the_oldest_handovers_are_forgotten_first() {
        let mut log = Handovers::default();
        for n in 0..(MAX_REMEMBERED as i64 + 5) {
            log.insert(&n.to_string(), handover(n));
        }
        assert!(log.get("4").is_none());
        assert!(log.get("5").is_some());
        assert_eq!(log.known.len(), MAX_REMEMBERED);
        assert_eq!(log.order.len(), MAX_REMEMBERED);
    }

    #[test]
    fn retarget_keeps_the_first_answer() {
        let mut log = Handovers::default();
        log.insert("a", handover(1));
        log.retarget("a", Target::Dismissed);
        let known = log.get("a").unwrap();
        assert_eq!(known.target, Target::Dismissed);
        assert_eq!(known.reply.id, Some(DownloadId(1)));
        log.remove("a");
        assert!(log.get("a").is_none() && log.order.is_empty());
    }

    #[test]
    fn ids_are_checked() {
        assert!(check_id("").is_err());
        assert!(check_id(&"x".repeat(MAX_ID_LEN + 1)).is_err());
        assert!(check_id("8f1c-42").is_ok());
    }
}
