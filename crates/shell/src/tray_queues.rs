//! The tray menu's Queues submenu: each queue, checked while it runs, paused or resumed with
//! one click.

use wirecatch_core::Manager;
use wirecatch_protocol::QueueId;
use wirecatch_protocol::app::Queue;

use crate::api::{self, CommandResult};

const ID_PREFIX: &str = "queue:";

/// One entry of the submenu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueItem {
    pub id: String,
    pub label: String,
    /// The queue runs: clicking pauses it.
    pub checked: bool,
}

/// The submenu's entries, one per queue in sidebar order.
#[must_use]
pub fn items(queues: &[Queue]) -> Vec<QueueItem> {
    queues
        .iter()
        .map(|queue| QueueItem {
            id: format!("{ID_PREFIX}{}", queue.id.0),
            label: queue.settings.name.clone(),
            checked: !queue.status.paused,
        })
        .collect()
}

/// The queue a menu id names, or `None` for an id that is not one of the submenu's.
#[must_use]
pub fn queue_of(id: &str) -> Option<QueueId> {
    id.strip_prefix(ID_PREFIX)?.parse().ok().map(QueueId)
}

/// Pauses queue `id` when it runs, and resumes it when paused.
///
/// # Errors
///
/// The queue is gone, or the manager has shut down.
pub fn toggle(manager: &Manager, id: QueueId) -> CommandResult<()> {
    let paused = api::list_queues(manager)
        .iter()
        .find(|queue| queue.id == id)
        .is_some_and(|queue| queue.status.paused);
    api::set_queue_paused(manager, id, !paused)
}

#[cfg(test)]
mod tests {
    use wirecatch_protocol::app::{QueueSettings, QueueStatus};

    use super::*;

    fn queue(id: i64, name: &str, paused: bool) -> Queue {
        Queue {
            id: QueueId(id),
            is_default: id == 1,
            settings: QueueSettings::new(name),
            status: QueueStatus {
                paused,
                ..QueueStatus::default()
            },
        }
    }

    #[test]
    fn each_queue_is_listed_and_checked_while_it_runs() {
        let items = items(&[queue(1, "Main", false), queue(7, "Night", true)]);
        let shown: Vec<_> = items
            .iter()
            .map(|i| (i.label.as_str(), i.checked))
            .collect();
        assert_eq!(shown, [("Main", true), ("Night", false)]);
        assert_eq!(queue_of(&items[1].id), Some(QueueId(7)));
        assert_eq!(queue_of("speed:off"), None);
    }
}
