//! The tray menu's Speed limit submenu: the app-wide limit, switched with one click.

use wirecatch_core::Manager;
use wirecatch_engine::units::{KIB, MIB};
use wirecatch_protocol::app::{AppSettingsPatch, Update};

use crate::activity::format_bytes;
use crate::api::{self, CommandResult};

const ID_PREFIX: &str = "speed:";
const NO_LIMIT: &str = "off";

/// The limits offered besides none, in bytes per second.
const OFFERED: [u64; 5] = [256 * KIB, 512 * KIB, MIB, 5 * MIB, 10 * MIB];

/// One entry of the submenu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpeedItem {
    pub id: String,
    pub label: String,
    pub checked: bool,
}

/// The submenu's entries: no limit, the offered limits and the app's own when it is another,
/// the app's checked.
#[must_use]
pub fn items(limit: Option<u64>) -> Vec<SpeedItem> {
    let mut limits: Vec<Option<u64>> = std::iter::once(None)
        .chain(OFFERED.into_iter().map(Some))
        .collect();
    if !limits.contains(&limit) {
        limits.push(limit);
        limits[1..].sort_unstable();
    }
    limits
        .into_iter()
        .map(|each| SpeedItem {
            id: id_of(each),
            label: each.map_or_else(
                || "No limit".to_owned(),
                |bytes| format!("{}/s", format_bytes(bytes)),
            ),
            checked: each == limit,
        })
        .collect()
}

fn id_of(limit: Option<u64>) -> String {
    match limit {
        None => format!("{ID_PREFIX}{NO_LIMIT}"),
        Some(bytes) => format!("{ID_PREFIX}{bytes}"),
    }
}

/// The limit a menu id picks (`Some(None)` for no limit), or `None` for an id that is not one of
/// the submenu's.
#[must_use]
pub fn limit_of(id: &str) -> Option<Option<u64>> {
    match id.strip_prefix(ID_PREFIX)? {
        NO_LIMIT => Some(None),
        bytes => bytes.parse().ok().filter(|b| *b > 0).map(Some),
    }
}

/// Makes `limit` the app's speed limit.
///
/// # Errors
///
/// The manager has shut down.
pub fn switch(manager: &Manager, limit: Option<u64>) -> CommandResult<()> {
    api::change_settings(
        manager,
        AppSettingsPatch {
            global_speed_limit: Update::Set(limit),
            ..AppSettingsPatch::default()
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_submenu_offers_limits_and_checks_the_apps() {
        let items = items(Some(MIB));
        let shown: Vec<_> = items
            .iter()
            .map(|i| (i.label.as_str(), i.checked))
            .collect();
        assert_eq!(
            shown,
            [
                ("No limit", false),
                ("256 KB/s", false),
                ("512 KB/s", false),
                ("1.00 MB/s", true),
                ("5.00 MB/s", false),
                ("10.0 MB/s", false),
            ]
        );
        for item in &items {
            assert!(limit_of(&item.id).is_some(), "{}", item.id);
        }
    }

    #[test]
    fn a_limit_of_the_persons_own_is_listed_in_its_place() {
        let items = items(Some(2 * MIB));
        assert_eq!(items[4].label, "2.00 MB/s");
        assert!(items[4].checked);
        assert_eq!(items.iter().filter(|i| i.checked).count(), 1);
        assert!(super::items(None)[0].checked);
    }

    #[test]
    fn ids_read_back_to_their_limit() {
        assert_eq!(limit_of(&id_of(None)), Some(None));
        assert_eq!(limit_of(&id_of(Some(5 * MIB))), Some(Some(5 * MIB)));
        assert_eq!(limit_of("speed:0"), None);
        assert_eq!(limit_of("proxy:direct"), None);
    }
}
