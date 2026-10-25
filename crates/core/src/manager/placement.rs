//! Which queue a new download joins; every way a download is added asks here.

use wirecatch_protocol::QueueId;

use super::state::State;
use crate::error::Result;

/// Who names a new download's queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum QueueRequest {
    /// The person picked it in the app.
    Picked(QueueId),
    /// Nobody did, for a download added in the app.
    App,
    /// A download from the browser, with the queue the extension named, if any.
    Browser(Option<QueueId>),
}

impl QueueRequest {
    /// The request of a download added in the app, which names its queue or not.
    pub fn in_app(picked: Option<QueueId>) -> Self {
        picked.map_or(Self::App, Self::Picked)
    }
}

impl State {
    /// The queue a new download joins: the one picked, else for a browser download the one
    /// the extension named or else the browser's queue, else the default queue.
    ///
    /// # Errors
    ///
    /// [`CoreError::QueueNotFound`](crate::CoreError::QueueNotFound) for a picked queue that
    /// is gone. A browser's queue deleted since the extension last synced is no error: the
    /// download joins the default queue.
    pub fn queue_for(&self, request: QueueRequest) -> Result<QueueId> {
        match request {
            QueueRequest::Picked(queue) => Ok(self.slot(queue)?.queue.id),
            QueueRequest::App => Ok(self.default_queue()),
            QueueRequest::Browser(named) => {
                let wanted = named.unwrap_or(self.settings.extension.default_queue);
                Ok(self
                    .slot(wanted)
                    .map_or_else(|_| self.default_queue(), |s| s.queue.id))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use wirecatch_protocol::app::{Queue, QueueSettings};

    use super::*;
    use crate::error::CoreError;
    use crate::manager::restore::first_start;
    use crate::manager::state::QueueSlot;
    use crate::settings::{DEFAULT_QUEUE_ID, default_queue};

    const OTHER: QueueId = QueueId(7);
    const GONE: QueueId = QueueId(99);

    fn state_with_browser_queue(browser: QueueId) -> State {
        let mut state = first_start();
        let main = default_queue();
        state.queues.push(QueueSlot::new(Queue {
            id: OTHER,
            is_default: false,
            settings: QueueSettings {
                name: "Other".into(),
                ..main.settings
            },
            ..main
        }));
        state.settings.extension.default_queue = browser;
        state
    }

    #[test]
    fn a_picked_queue_is_taken_and_one_that_is_gone_is_refused() {
        let state = state_with_browser_queue(OTHER);
        assert_eq!(state.queue_for(QueueRequest::Picked(OTHER)).unwrap(), OTHER);
        assert!(matches!(
            state.queue_for(QueueRequest::Picked(GONE)),
            Err(CoreError::QueueNotFound(GONE))
        ));
    }

    #[test]
    fn a_download_added_in_the_app_without_a_queue_joins_the_default_queue() {
        let state = state_with_browser_queue(OTHER);
        assert_eq!(
            state.queue_for(QueueRequest::in_app(None)).unwrap(),
            DEFAULT_QUEUE_ID
        );
    }

    #[test]
    fn a_browser_download_joins_the_named_queue_then_the_browsers_then_the_default() {
        let state = state_with_browser_queue(OTHER);
        let browser = |named| state.queue_for(QueueRequest::Browser(named)).unwrap();
        assert_eq!(browser(Some(DEFAULT_QUEUE_ID)), DEFAULT_QUEUE_ID);
        assert_eq!(browser(None), OTHER);
        assert_eq!(browser(Some(GONE)), DEFAULT_QUEUE_ID);
        let browser_queue_gone = state_with_browser_queue(GONE);
        assert_eq!(
            browser_queue_gone
                .queue_for(QueueRequest::Browser(None))
                .unwrap(),
            DEFAULT_QUEUE_ID
        );
    }
}
