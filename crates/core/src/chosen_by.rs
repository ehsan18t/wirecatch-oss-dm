//! Who chose a download's link, which decides whether its requests may reach
//! the local network (D34.5).

use serde::{Deserialize, Serialize};
use wirecatch_engine::PrivateNetwork;

/// Who chose a download's link. It is set once, where the link arrives, kept
/// with the download, and is the only thing that decides the rule for the
/// local network, so no single call site can pick the wrong one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ChosenBy {
    /// The person typed or pasted it into Wirecatch, so it may name a server
    /// on their own network: a NAS or an intranet host.
    Person,
    /// A website: the browser extension captured it (a download, a stream or a
    /// video page). Its owner chooses where its name leads, so it may never
    /// lead into the local network.
    Website,
}

impl ChosenBy {
    /// The engine's rule for the local network. [`PrivateNetwork::Block`]
    /// lets a website's link through only when its host is written as an
    /// address (D34.4), which a name cannot be rebound to.
    pub(crate) fn private_network(self) -> PrivateNetwork {
        match self {
            Self::Person => PrivateNetwork::Auto,
            Self::Website => PrivateNetwork::Block,
        }
    }
}
