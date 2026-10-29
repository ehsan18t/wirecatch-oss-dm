use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use wirecatch_protocol::proxy::{ProxyServerSettings, ProxyTestRun};
use wirecatch_protocol::{ProxyServerId, Secret};

/// The passwords of the proxy servers, which never leave the app (D71.2).
pub type Logins = BTreeMap<ProxyServerId, Secret>;

/// A proxy server as it is saved, without its password.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedProxyServer {
    pub id: ProxyServerId,
    pub settings: ProxyServerSettings,
    pub last_test: Option<ProxyTestRun>,
}
