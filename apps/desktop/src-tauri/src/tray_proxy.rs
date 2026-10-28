//! The tray menu's Proxy submenu (D73.4): the app's proxy, switched with one click.

use wirecatch_core::Manager;
use wirecatch_protocol::ProxyProfileId;
use wirecatch_protocol::app::AppSettingsPatch;
use wirecatch_protocol::proxy::{ProxyProfile, ProxySettings, ProxyUse};

use crate::api;

const ID_PREFIX: &str = "proxy:";

/// One entry of the submenu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyItem {
    pub id: String,
    pub label: String,
    pub checked: bool,
}

/// The submenu's entries: the computer's proxy, no proxy and each saved profile, the app's
/// own checked.
#[must_use]
pub fn items(app: &ProxySettings, profiles: &[ProxyProfile]) -> Vec<ProxyItem> {
    let fixed = [
        (ProxyUse::System, "The computer's proxy".to_owned()),
        (ProxyUse::Direct, "No proxy".to_owned()),
    ];
    let saved = profiles
        .iter()
        .map(|p| (ProxyUse::Profile(p.id), p.settings.shown_name()));
    fixed
        .into_iter()
        .chain(saved)
        .map(|(proxy, label)| ProxyItem {
            id: id_of(proxy),
            label,
            checked: proxy == app.proxy,
        })
        .collect()
}

fn id_of(proxy: ProxyUse) -> String {
    match proxy {
        ProxyUse::System => format!("{ID_PREFIX}system"),
        ProxyUse::Direct => format!("{ID_PREFIX}direct"),
        ProxyUse::Profile(id) => format!("{ID_PREFIX}{}", id.0),
    }
}

/// The proxy a menu id picks, or `None` for an id that is not one of the submenu's.
#[must_use]
pub fn proxy_of(id: &str) -> Option<ProxyUse> {
    match id.strip_prefix(ID_PREFIX)? {
        "system" => Some(ProxyUse::System),
        "direct" => Some(ProxyUse::Direct),
        n => n.parse().ok().map(|n| ProxyUse::Profile(ProxyProfileId(n))),
    }
}

/// Makes `proxy` the app's.
///
/// # Errors
///
/// The profile is gone, or the manager has shut down.
pub fn switch(manager: &Manager, proxy: ProxyUse) -> Result<(), String> {
    let mut settings = manager.get_settings().proxy;
    settings.proxy = proxy;
    api::change_settings(
        manager,
        AppSettingsPatch {
            proxy: Some(settings),
            ..AppSettingsPatch::default()
        },
    )
    .map_err(|e| e.message)
}

#[cfg(test)]
mod tests {
    use wirecatch_protocol::proxy::{ProxyKind, ProxyProfileSettings};

    use super::*;

    fn profile(id: i64, name: &str) -> ProxyProfile {
        ProxyProfile {
            id: ProxyProfileId(id),
            settings: ProxyProfileSettings {
                name: name.into(),
                kind: ProxyKind::Http,
                host: "proxy.example".into(),
                port: 8080,
                user: String::new(),
            },
            has_password: false,
            last_test: None,
        }
    }

    #[test]
    fn the_submenu_lists_every_proxy_and_checks_the_apps() {
        let app = ProxySettings {
            proxy: ProxyUse::Profile(ProxyProfileId(4)),
            ..ProxySettings::default()
        };
        let items = items(&app, &[profile(4, "Office"), profile(5, "")]);
        let shown: Vec<_> = items
            .iter()
            .map(|i| (i.label.as_str(), i.checked))
            .collect();
        assert_eq!(
            shown,
            [
                ("The computer's proxy", false),
                ("No proxy", false),
                ("Office", true),
                ("proxy.example:8080", false),
            ]
        );
        for item in &items {
            assert!(proxy_of(&item.id).is_some(), "{}", item.id);
        }
        assert_eq!(
            proxy_of(&items[2].id),
            Some(ProxyUse::Profile(ProxyProfileId(4)))
        );
        assert_eq!(proxy_of("pause_all"), None);
        assert_eq!(proxy_of("proxy:nonsense"), None);
    }
}
