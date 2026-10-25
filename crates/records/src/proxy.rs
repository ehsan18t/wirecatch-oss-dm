use std::collections::BTreeMap;

use wirecatch_protocol::Secret;
use wirecatch_protocol::proxy::ProxyRole;

/// The passwords of the manual proxies, which never leave the app (D71.2).
pub type Logins = BTreeMap<ProxyRole, Secret>;
