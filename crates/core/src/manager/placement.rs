use wirecatch_engine::Url;
use wirecatch_protocol::QueueId;
use wirecatch_protocol::app::Category;
use wirecatch_protocol::rules::{AddDownloads, DraftRules, LinkMatch, QueueReason, RuleMatch};
use wirecatch_queues::rules::{self, Found, Link, Matched, RuleQueue};
use wirecatch_sites::local_network::is_on_own_network;

use super::state::State;
use crate::error::Result;
use crate::naming;

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

/// A new download as the rules read it (D72.3).
#[derive(Debug, Clone, Copy)]
pub(super) struct Candidate<'a> {
    pub url: &'a Url,
    /// The name known when it is added: the browser's for a captured file, else the link's.
    pub file_name: &'a str,
    /// `None`: the category Settings sorts the name into.
    pub category: Option<Category>,
}

impl<'a> Candidate<'a> {
    /// A video is matched by its page's address and the name its window proposes (D72.11).
    pub fn video(page: &'a Url, file_name: &'a str) -> Self {
        Self {
            url: page,
            file_name,
            category: Some(Category::Video),
        }
    }
}

impl State {
    /// The person's pick, else the first rule that applies (D72.3), else the browser's queue for a browser download, else the default queue. A picked queue that is gone is an error; a browser's queue deleted since the extension synced gives way to the default queue.
    pub fn place(&self, request: QueueRequest, candidate: &Candidate<'_>) -> Result<Placed> {
        if let QueueRequest::Picked(queue) = request {
            return Ok(Placed::picked(self.slot(queue)?.queue.id));
        }
        if let Some(found) = self.rule(candidate).matched {
            return Ok(Placed {
                queue: found.queue,
                reason: QueueReason::Matched(found.rule),
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
    pub fn rule(&self, candidate: &Candidate<'_>) -> Matched {
        let auto: Vec<RuleQueue<'_>> = self
            .queues
            .iter()
            .filter_map(|slot| auto_queue(slot.queue.id, &slot.queue.settings.add_downloads))
            .collect();
        self.rule_among(&auto, candidate)
    }

    /// What the rules make of `url` for Try a link: the saved rules, or `draft` in its queue's place, a queue not made yet after every saved one (D72.7).
    pub fn try_link(&self, url: &Url, draft: Option<&DraftRules>) -> LinkMatch {
        let drafted = |id: QueueId| {
            draft
                .filter(|d| d.queue == Some(id))
                .map(|d| &d.add_downloads)
        };
        let mut auto: Vec<RuleQueue<'_, Option<QueueId>>> = self
            .queues
            .iter()
            .filter_map(|slot| {
                let id = slot.queue.id;
                auto_queue(
                    Some(id),
                    drafted(id).unwrap_or(&slot.queue.settings.add_downloads),
                )
            })
            .collect();
        if let Some(new) = draft.filter(|d| d.queue.is_none()) {
            auto.extend(auto_queue(None, &new.add_downloads));
        }
        let name = naming::file_name(None, url).unwrap_or_default();
        let candidate = Candidate {
            url,
            file_name: &name,
            category: None,
        };
        let matched = self.rule_among(&auto, &candidate);
        let tried = |found: Found<Option<QueueId>>| RuleMatch {
            queue: found.queue,
            rule: found.rule,
        };
        LinkMatch {
            matched: matched.matched.map(tried),
            switched_off: matched.switched_off.map(tried),
        }
    }

    fn rule_among<K: Copy + PartialEq>(
        &self,
        queues: &[RuleQueue<'_, K>],
        candidate: &Candidate<'_>,
    ) -> Matched<K> {
        let category = candidate.category.unwrap_or_else(|| {
            wirecatch_settings::category_of(&self.settings, candidate.file_name)
        });
        let link = Link {
            url: candidate.url,
            file_name: candidate.file_name,
            category,
        };
        rules::find(queues, &link, &|url| {
            is_on_own_network(url, &self.settings.local_sites, &self.suffixes)
        })
    }
}

fn auto_queue<K>(id: K, add_downloads: &AddDownloads) -> Option<RuleQueue<'_, K>> {
    match add_downloads {
        AddDownloads::Auto(rules) => Some(RuleQueue { id, rules }),
        AddDownloads::Manual => None,
    }
}

#[cfg(test)]
mod tests {
    use wirecatch_protocol::app::{Queue, QueueSettings};
    use wirecatch_protocol::rules::{QueueRules, Rule};

    use super::*;
    use crate::error::CoreError;
    use crate::manager::restore::first_start;
    use crate::manager::state::QueueSlot;
    use wirecatch_settings::{DEFAULT_QUEUE_ID, default_queue};

    const OTHER: QueueId = QueueId(7);
    const VIDEOS: QueueId = QueueId(8);
    const GONE: QueueId = QueueId(99);

    fn queue(id: QueueId, name: &str, add_downloads: AddDownloads) -> QueueSlot {
        QueueSlot::new(Queue {
            id,
            is_default: false,
            settings: QueueSettings {
                add_downloads,
                ..QueueSettings::new(name)
            },
            ..default_queue()
        })
    }

    /// Main, a manual queue the browser's downloads go to, and an auto queue that rules `example.com` while it is `on`.
    fn state_with(browser: QueueId, on: bool) -> State {
        let mut state = first_start();
        state
            .queues
            .push(queue(OTHER, "Other", AddDownloads::Manual));
        let rules = QueueRules {
            enabled: on,
            sites: vec!["example.com".into()],
            ..QueueRules::default()
        };
        state
            .queues
            .push(queue(VIDEOS, "Videos", AddDownloads::Auto(rules)));
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
    const RULEED: &str = "https://cdn.example.com/a.bin";

    #[test]
    fn a_picked_queue_is_taken_over_any_rule_and_one_that_is_gone_is_refused() {
        let state = state_with(OTHER, true);
        assert_eq!(
            place(&state, QueueRequest::Picked(OTHER), RULEED).unwrap(),
            Placed::picked(OTHER)
        );
        assert!(matches!(
            place(&state, QueueRequest::Picked(GONE), RULEED),
            Err(CoreError::QueueNotFound(GONE))
        ));
    }

    #[test]
    fn a_rule_comes_before_the_browsers_queue_and_the_default_queue() {
        let state = state_with(OTHER, true);
        let matched = at(
            VIDEOS,
            QueueReason::Matched(Rule::Site("example.com".into())),
        );
        assert_eq!(
            place(&state, QueueRequest::Browser(None), RULEED).unwrap(),
            matched
        );
        assert_eq!(place(&state, QueueRequest::App, RULEED).unwrap(), matched);
    }

    #[test]
    fn a_queue_that_is_off_rules_nothing() {
        let state = state_with(OTHER, false);
        assert_eq!(
            place(&state, QueueRequest::App, RULEED).unwrap(),
            at(DEFAULT_QUEUE_ID, QueueReason::DefaultQueue)
        );
        let url = Url::parse(RULEED).unwrap();
        let candidate = Candidate {
            url: &url,
            file_name: "a.bin",
            category: None,
        };
        assert_eq!(
            state.rule(&candidate).switched_off.map(|f| f.queue),
            Some(VIDEOS)
        );
    }

    #[test]
    fn unmatched_downloads_join_the_browsers_queue_then_the_default_queue() {
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

    fn tried(queue: Option<QueueId>, site: &str) -> RuleMatch {
        RuleMatch {
            queue,
            rule: Rule::Site(site.into()),
        }
    }

    fn sites(on: bool, sites: &[&str]) -> AddDownloads {
        AddDownloads::Auto(QueueRules {
            enabled: on,
            sites: sites.iter().map(ToString::to_string).collect(),
            ..QueueRules::default()
        })
    }

    #[test]
    fn try_a_link_answers_by_the_saved_rules_or_one_queues_unsaved_ones() {
        let state = state_with(OTHER, true);
        let url = Url::parse(RULEED).unwrap();
        let saved = state.try_link(&url, None);
        assert_eq!(saved.matched, Some(tried(Some(VIDEOS), "example.com")));

        let other_drafted = DraftRules {
            queue: Some(OTHER),
            add_downloads: sites(true, &["cdn.example.com"]),
        };
        let drafted = state.try_link(&url, Some(&other_drafted));
        assert_eq!(drafted.matched, Some(tried(Some(OTHER), "cdn.example.com")));

        let videos_off = DraftRules {
            queue: Some(VIDEOS),
            add_downloads: sites(false, &["example.com"]),
        };
        let off = state.try_link(&url, Some(&videos_off));
        assert_eq!(off.matched, None);
        assert_eq!(off.switched_off, Some(tried(Some(VIDEOS), "example.com")));

        let new_queue = DraftRules {
            queue: None,
            add_downloads: sites(true, &["example.com"]),
        };
        let tie = state.try_link(&url, Some(&new_queue));
        assert_eq!(
            tie.matched,
            Some(tried(Some(VIDEOS), "example.com")),
            "a saved queue wins a tie"
        );
        let new_wins = state.try_link(
            &url,
            Some(&DraftRules {
                add_downloads: sites(true, &["cdn.example.com"]),
                ..new_queue
            }),
        );
        assert_eq!(new_wins.matched, Some(tried(None, "cdn.example.com")));
    }

    #[test]
    fn a_picked_queue_in_a_window_is_the_persons_pick_only_when_it_changed() {
        let proposed = at(
            VIDEOS,
            QueueReason::Matched(Rule::Site("example.com".into())),
        );
        assert_eq!(Placed::answered(proposed.clone(), None), proposed);
        assert_eq!(Placed::answered(proposed.clone(), Some(VIDEOS)), proposed);
        assert_eq!(
            Placed::answered(proposed, Some(OTHER)),
            Placed::picked(OTHER)
        );
    }
}
