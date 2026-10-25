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
    /// A queue that is gone leaves the app's limit alone.
    pub(super) fn run_policy(&self, state: &State, queue: QueueId) -> RunPolicy {
        let queue_limiter = state.slot(queue).ok().map(|s| s.limiter.clone());
        RunPolicy {
            limiters: [Some(self.inner.global_limiter.clone()), queue_limiter]
                .into_iter()
                .flatten()
                .collect(),
            ..self.app_policy(state)
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
