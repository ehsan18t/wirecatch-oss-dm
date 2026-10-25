use std::sync::Arc;

use wirecatch_engine::{ProxyChoice, RateLimiter};
use wirecatch_protocol::QueueId;

use super::Manager;
use super::state::State;

pub(super) struct RunPolicy {
    pub connections: usize,
    /// The app's, then the queue's.
    pub limiters: Vec<Arc<RateLimiter>>,
    pub proxy: ProxyChoice,
}

impl Manager {
    /// Reading a stream or page before the run takes only the connections and proxy, unlimited.
    /// A queue that is gone leaves the app's limit and connections alone.
    pub(super) fn run_policy(&self, state: &State, queue: QueueId) -> RunPolicy {
        let app = self.app_policy(state);
        let Ok(slot) = state.slot(queue) else {
            return app;
        };
        RunPolicy {
            connections: slot
                .queue
                .settings
                .connections
                .own()
                .map_or(app.connections, |own| usize::from(own).max(1)),
            limiters: vec![self.inner.global_limiter.clone(), slot.limiter.clone()],
            ..app
        }
    }

    pub(super) fn app_policy(&self, state: &State) -> RunPolicy {
        RunPolicy {
            connections: usize::from(state.settings.connections_per_download).max(1),
            limiters: vec![self.inner.global_limiter.clone()],
            proxy: state.app_proxy(),
        }
    }
}
