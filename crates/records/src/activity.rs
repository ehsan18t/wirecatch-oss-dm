//! What happened to a download after it was added, for its Details (D236).

use serde::{Deserialize, Serialize};
use wirecatch_protocol::UnixMillis;

/// How many events a download keeps; older ones go first.
pub const EVENTS_KEPT: usize = 50;

/// One thing that happened to a download.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    /// It failed, with the message the person was shown.
    Failed {
        message: String,
    },
    Paused,
    /// It went in line for the first time, added paused.
    Started,
    /// It went back in line after a pause.
    Resumed,
    /// It went back in line after it failed or was cancelled.
    Retried,
    Cancelled,
    /// It was given a new link: the hosts of the link before and after.
    LinkChanged {
        from: String,
        to: String,
    },
    /// Its files moved to this folder.
    Moved {
        folder: String,
    },
}

/// An [`Event`] and when it happened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Happened {
    pub at: UnixMillis,
    pub event: Event,
}

/// A download's events, oldest first, at most [`EVENTS_KEPT`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Activity(Vec<Happened>);

impl Activity {
    /// Adds `event`, dropping the oldest past [`EVENTS_KEPT`].
    pub fn record(&mut self, at: UnixMillis, event: Event) {
        if self.0.len() >= EVENTS_KEPT {
            let over = self.0.len() + 1 - EVENTS_KEPT;
            self.0.drain(..over);
        }
        self.0.push(Happened { at, event });
    }

    #[must_use]
    pub fn events(&self) -> &[Happened] {
        &self.0
    }

    /// Whether the last pause, start or stop it records is a pause.
    #[must_use]
    pub fn paused_last(&self) -> bool {
        self.0
            .iter()
            .rev()
            .find(|h| !matches!(h.event, Event::LinkChanged { .. } | Event::Moved { .. }))
            .is_some_and(|h| h.event == Event::Paused)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_latest_events_are_kept() {
        let mut activity = Activity::default();
        for at in 0..=i64::try_from(EVENTS_KEPT).unwrap() {
            activity.record(UnixMillis(at), Event::Paused);
        }
        assert_eq!(activity.events().len(), EVENTS_KEPT);
        assert_eq!(activity.events()[0].at, UnixMillis(1));
    }

    #[test]
    fn events_are_saved_by_kind() {
        let mut activity = Activity::default();
        activity.record(
            UnixMillis(5),
            Event::Failed {
                message: "The disk is full.".into(),
            },
        );
        let json = serde_json::to_string(&activity).unwrap();
        assert_eq!(
            json,
            r#"[{"at":5,"event":{"kind":"failed","message":"The disk is full."}}]"#
        );
        assert_eq!(serde_json::from_str::<Activity>(&json).unwrap(), activity);
    }
}
