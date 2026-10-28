use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use wirecatch_protocol::proxy::{ProxyProfileSettings, ProxyTestRun};
use wirecatch_protocol::{ProxyProfileId, Secret};

/// The passwords of the proxy profiles, which never leave the app (D71.2).
pub type Logins = BTreeMap<ProxyProfileId, Secret>;

/// A proxy profile as it is saved, without its password.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedProfile {
    pub id: ProxyProfileId,
    pub settings: ProxyProfileSettings,
    pub last_test: Option<ProxyTestRun>,
}
