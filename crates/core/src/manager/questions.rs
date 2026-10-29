//! Captures and videos waiting for the person's answer (CORE-5): the limits and the rules for the browser handovers they hold, kept once for both kinds.

use std::collections::BTreeMap;
use std::ops::{Deref, DerefMut};

use wirecatch_protocol::QuestionId;

use crate::error::{CoreError, Result};

/// Most questions of one kind that can wait at once.
const MAX_WAITING: usize = 100;
/// Most browser handovers of one link that can share one question.
pub(super) const MAX_HANDOVERS: usize = 32;

/// A question that holds the extension's handover ids: one per capture of its link, from several tabs. It goes away only when every one is withdrawn.
pub(super) trait Waiting {
    fn handovers(&mut self) -> &mut Vec<String>;
}

/// The questions of one kind, by id. Ids come from one counter for every kind, so a question id names one question.
pub(super) struct Questions<T> {
    waiting: BTreeMap<QuestionId, T>,
    /// What the person is told when [`MAX_WAITING`] are waiting, after that number.
    full: &'static str,
    /// What the person is told when a question holds [`MAX_HANDOVERS`].
    crowded: &'static str,
}

/// What withdrawing a handover from a question did.
pub(super) enum Withdrawn<T> {
    /// No question with that id is of this kind.
    NotHere,
    /// Other handovers still wait on it.
    StillAsked,
    /// It was the last one: the question is gone.
    Closed(T),
}

impl<T: Waiting> Questions<T> {
    pub fn new(full: &'static str, crowded: &'static str) -> Self {
        Self {
            waiting: BTreeMap::new(),
            full,
            crowded,
        }
    }

    /// Refuses one more question while [`MAX_WAITING`] are waiting.
    pub fn check_room(&self) -> Result<()> {
        if self.waiting.len() >= MAX_WAITING {
            return Err(CoreError::LimitReached(format!(
                "{MAX_WAITING} {}",
                self.full
            )));
        }
        Ok(())
    }

    /// Adds `handover` to question `id`, unless it already holds [`MAX_HANDOVERS`].
    pub fn join(&mut self, id: QuestionId, handover: &str) -> Result<()> {
        let Some(question) = self.waiting.get_mut(&id) else {
            return Ok(());
        };
        let handovers = question.handovers();
        if handovers.len() >= MAX_HANDOVERS {
            return Err(CoreError::LimitReached(self.crowded.into()));
        }
        handovers.push(handover.to_string());
        Ok(())
    }

    /// Takes `handover` off question `id`; the question closes with its last one.
    pub fn withdraw(&mut self, id: QuestionId, handover: &str) -> Withdrawn<T> {
        let Some(question) = self.waiting.get_mut(&id) else {
            return Withdrawn::NotHere;
        };
        let handovers = question.handovers();
        handovers.retain(|h| h != handover);
        if !handovers.is_empty() {
            return Withdrawn::StillAsked;
        }
        self.waiting
            .remove(&id)
            .map_or(Withdrawn::NotHere, Withdrawn::Closed)
    }
}

impl<T> Deref for Questions<T> {
    type Target = BTreeMap<QuestionId, T>;

    fn deref(&self) -> &Self::Target {
        &self.waiting
    }
}

impl<T> DerefMut for Questions<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.waiting
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Asked(Vec<String>);

    impl Waiting for Asked {
        fn handovers(&mut self) -> &mut Vec<String> {
            &mut self.0
        }
    }

    fn questions() -> Questions<Asked> {
        Questions::new("full", "crowded")
    }

    #[test]
    fn a_question_closes_with_its_last_handover() {
        let mut asked = questions();
        asked.insert(QuestionId(1), Asked(vec!["a".into()]));
        asked.join(QuestionId(1), "b").unwrap();
        assert!(matches!(
            asked.withdraw(QuestionId(1), "a"),
            Withdrawn::StillAsked
        ));
        assert!(matches!(
            asked.withdraw(QuestionId(1), "b"),
            Withdrawn::Closed(_)
        ));
        assert!(matches!(
            asked.withdraw(QuestionId(1), "b"),
            Withdrawn::NotHere
        ));
    }

    #[test]
    fn both_limits_refuse() {
        let mut asked = questions();
        asked.insert(QuestionId(1), Asked(Vec::new()));
        for n in 0..MAX_HANDOVERS {
            asked.join(QuestionId(1), &n.to_string()).unwrap();
        }
        assert!(
            matches!(asked.join(QuestionId(1), "one more"), Err(CoreError::LimitReached(m)) if m == "crowded")
        );
        for n in 2..=MAX_WAITING {
            asked.insert(QuestionId(n as u64), Asked(Vec::new()));
        }
        assert!(
            matches!(asked.check_room(), Err(CoreError::LimitReached(m)) if m == format!("{MAX_WAITING} full"))
        );
    }
}
