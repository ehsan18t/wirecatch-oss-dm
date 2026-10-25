use wirecatch_engine::Url;
use wirecatch_protocol::QueueId;
use wirecatch_protocol::app::Category;
use wirecatch_protocol::claims::{QueueReason, QueueTakes};
use wirecatch_queues::claims::{self, Claimed, ClaimingQueue, Link};
use wirecatch_sites::local_network::is_on_own_network;

use super::state::State;
use crate::error::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum QueueRequest {
    Picked(QueueId),
    /// Added in the app with no queue picked.
    App,
    /// With the queue the extension named, if any.
    Browser(Option<QueueId>),
}

impl QueueRequest {
    pub fn in_app(picked: Option<QueueId>) -> Self {
        picked.map_or(Self::App, Self::Picked)
    }
}

/// The queue a new download joins, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Placed {
    pub queue: QueueId,
    pub reason: QueueReason,
}

impl Placed {
    pub fn picked(queue: QueueId) -> Self {
        Self {
            queue,
            reason: QueueReason::Picked,
        }
    }

    /// What placing it again asks for: the same queue if the person picked it.
    pub fn request(&self) -> QueueRequest {
        if self.reason == QueueReason::Picked {
            QueueRequest::Picked(self.queue)
        } else {
            QueueRequest::App
        }
    }

    /// `proposed` unless the person picked another queue in its window (D72.8).
    pub fn answered(proposed: Self, answer: Option<QueueId>) -> Self {
        match answer {
            Some(queue) if queue != proposed.queue => Self::picked(queue),
            _ => proposed,
        }
    }
}

/// A new download as the claims read it (D72.3).
#[derive(Debug, Clone, Copy)]
pub(super) struct Candidate<'a> {
    pub url: &'a Url,
    /// The name known when it is added: the browser's for a captured file, else the link's.
    pub file_name: &'a str,
    /// `None`: the category Settings sorts the name into.
    pub category: Option<Category>,
}

impl<'a> Candidate<'a> {
    /// A video is claimed by its page's address and the name its window proposes (D72.11).
    pub fn video(page: &'a Url, file_name: &'a str) -> Self {
        Self {
            url: page,
            file_name,
            category: Some(Category::Video),
        }
    }
}

impl State {
    /// The person's pick, else the first claim that applies (D72.3), else the browser's queue for a browser download, else the default queue. A picked queue that is gone is an error; a browser's queue deleted since the extension synced gives way to the default queue.
    pub fn place(&self, request: QueueRequest, candidate: &Candidate<'_>) -> Result<Placed> {
        if let QueueRequest::Picked(queue) = request {
            return Ok(Placed::picked(self.slot(queue)?.queue.id));
        }
        if let Some(found) = self.claim(candidate).taken {
            return Ok(Placed {
                queue: found.queue,
                reason: QueueReason::Claimed(found.claim),
            });
        }
        let default = || Placed {
            queue: self.default_queue(),
            reason: QueueReason::DefaultQueue,
        };
        Ok(match request {
            QueueRequest::Browser(named) => {
                let wanted = named.unwrap_or(self.settings.extension.default_queue);
                self.slot(wanted).map_or_else(
                    |_| default(),
                    |s| Placed {
                        queue: s.queue.id,
                        reason: QueueReason::BrowserQueue,
                    },
                )
            }
            QueueRequest::App | QueueRequest::Picked(_) => default(),
        })
    }

    /// What the auto queues make of `candidate`, in sidebar order.
    pub fn claim(&self, candidate: &Candidate<'_>) -> Claimed {
        let auto: Vec<ClaimingQueue<'_>> = self
            .queues
            .iter()
            .filter_map(|slot| match &slot.queue.settings.takes {
                QueueTakes::Auto(claims) => Some(ClaimingQueue {
                    id: slot.queue.id,
                    claims,
                }),
                QueueTakes::Manual => None,
            })
            .collect();
        let category = candidate.category.unwrap_or_else(|| {
            wirecatch_settings::category_of(&self.settings, candidate.file_name)
        });
        let link = Link {
            url: candidate.url,
            file_name: candidate.file_name,
            category,
        };
        claims::claim(&auto, &link, &|url| {
            is_on_own_network(url, &self.settings.local_sites, &self.suffixes)
        })
    }
}

#[cfg(test)]
mod tests {
    use wirecatch_protocol::app::{Queue, QueueSettings};
    use wirecatch_protocol::claims::{Claim, QueueClaims};

    use super::*;
    use crate::error::CoreError;
    use crate::manager::restore::first_start;
    use crate::manager::state::QueueSlot;
    use wirecatch_settings::{DEFAULT_QUEUE_ID, default_queue};

    const OTHER: QueueId = QueueId(7);
    const VIDEOS: QueueId = QueueId(8);
    const GONE: QueueId = QueueId(99);

    fn queue(id: QueueId, name: &str, takes: QueueTakes) -> QueueSlot {
        QueueSlot::new(Queue {
            id,
            is_default: false,
            settings: QueueSettings {
                takes,
                ..QueueSettings::new(name)
            },
            ..default_queue()
        })
    }

    /// Main, a manual queue the browser's downloads go to, and an auto queue that claims `example.com` while it is `on`.
    fn state_with(browser: QueueId, on: bool) -> State {
        let mut state = first_start();
        state.queues.push(queue(OTHER, "Other", QueueTakes::Manual));
        let claims = QueueClaims {
            enabled: on,
            sites: vec!["example.com".into()],
            ..QueueClaims::default()
        };
        state
            .queues
            .push(queue(VIDEOS, "Videos", QueueTakes::Auto(claims)));
        state.settings.extension.default_queue = browser;
        state
    }

    fn place(state: &State, request: QueueRequest, link: &str) -> Result<Placed> {
        let url = Url::parse(link).unwrap();
        let candidate = Candidate {
            url: &url,
            file_name: "a.bin",
            category: None,
        };
        state.place(request, &candidate)
    }

    fn at(queue: QueueId, reason: QueueReason) -> Placed {
        Placed { queue, reason }
    }

    const ELSEWHERE: &str = "https://files.example.org/a.bin";
    const CLAIMED: &str = "https://cdn.example.com/a.bin";

    #[test]
    fn a_picked_queue_is_taken_over_any_claim_and_one_that_is_gone_is_refused() {
        let state = state_with(OTHER, true);
        assert_eq!(
            place(&state, QueueRequest::Picked(OTHER), CLAIMED).unwrap(),
            Placed::picked(OTHER)
        );
        assert!(matches!(
            place(&state, QueueRequest::Picked(GONE), CLAIMED),
            Err(CoreError::QueueNotFound(GONE))
        ));
    }

    #[test]
    fn a_claim_comes_before_the_browsers_queue_and_the_default_queue() {
        let state = state_with(OTHER, true);
        let claimed = at(
            VIDEOS,
            QueueReason::Claimed(Claim::Site("example.com".into())),
        );
        assert_eq!(
            place(&state, QueueRequest::Browser(None), CLAIMED).unwrap(),
            claimed
        );
        assert_eq!(place(&state, QueueRequest::App, CLAIMED).unwrap(), claimed);
    }

    #[test]
    fn a_queue_that_is_off_claims_nothing() {
        let state = state_with(OTHER, false);
        assert_eq!(
            place(&state, QueueRequest::App, CLAIMED).unwrap(),
            at(DEFAULT_QUEUE_ID, QueueReason::DefaultQueue)
        );
        let url = Url::parse(CLAIMED).unwrap();
        let candidate = Candidate {
            url: &url,
            file_name: "a.bin",
            category: None,
        };
        assert_eq!(
            state.claim(&candidate).switched_off.map(|f| f.queue),
            Some(VIDEOS)
        );
    }

    #[test]
    fn unclaimed_downloads_join_the_browsers_queue_then_the_default_queue() {
        let state = state_with(OTHER, true);
        let browser = |named| place(&state, QueueRequest::Browser(named), ELSEWHERE).unwrap();
        assert_eq!(browser(None), at(OTHER, QueueReason::BrowserQueue));
        assert_eq!(
            browser(Some(GONE)),
            at(DEFAULT_QUEUE_ID, QueueReason::DefaultQueue)
        );
        assert_eq!(
            place(&state, QueueRequest::App, ELSEWHERE).unwrap(),
            at(DEFAULT_QUEUE_ID, QueueReason::DefaultQueue)
        );
        let browser_queue_gone = state_with(GONE, true);
        assert_eq!(
            place(&browser_queue_gone, QueueRequest::Browser(None), ELSEWHERE).unwrap(),
            at(DEFAULT_QUEUE_ID, QueueReason::DefaultQueue)
        );
    }

    #[test]
    fn a_picked_queue_in_a_window_is_the_persons_pick_only_when_it_changed() {
        let proposed = at(
            VIDEOS,
            QueueReason::Claimed(Claim::Site("example.com".into())),
        );
        assert_eq!(Placed::answered(proposed.clone(), None), proposed);
        assert_eq!(Placed::answered(proposed.clone(), Some(VIDEOS)), proposed);
        assert_eq!(
            Placed::answered(proposed, Some(OTHER)),
            Placed::picked(OTHER)
        );
    }
}
