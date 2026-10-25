//! The one place that decides what a request the manager sends may use of the network: its
//! connections, the speed limits it shares and the proxies it follows, for a download by its
//! queue, and for the app's own requests.

use std::sync::Arc;

use wirecatch_engine::{ProxyChoice, RateLimiter};
use wirecatch_protocol::QueueId;

use super::Manager;
use super::state::State;

/// What a run may use of the network.
pub(super) struct RunPolicy {
    /// Connections per download, at least one.
    pub connections: usize,
    /// The speed limits it shares: the app's, then its queue's.
    pub limiters: Vec<Arc<RateLimiter>>,
    /// The proxies its requests follow.
    pub proxy: ProxyChoice,
}

impl Manager {
    /// How a download in `queue` runs. Reading its stream or video page first takes the
    /// connections and the proxy only: that is not limited in speed. A queue that is gone
    /// leaves the app's limit alone.
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

    /// How the app's own downloads run, such as the media add-on's.
    pub(super) fn app_policy(&self, state: &State) -> RunPolicy {
        RunPolicy {
            connections: usize::from(state.settings.connections_per_download).max(1),
            limiters: vec![self.inner.global_limiter.clone()],
            proxy: state.app_proxy(),
        }
    }
}
