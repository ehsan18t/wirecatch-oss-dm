use std::collections::HashMap;

use wirecatch_protocol::{DownloadId, QueueId};

#[derive(Debug, Clone, Copy)]
pub struct QueueView {
    pub id: QueueId,
    /// `None`: no limit of the queue's own, only the app's.
    pub max_concurrent: Option<u32>,
    pub open: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activity {
    Waiting,
    Running,
    /// Not finished stopping; it still holds its slot.
    Stopping,
}

#[derive(Debug, Clone, Copy)]
pub struct DownloadView {
    pub id: DownloadId,
    pub queue: QueueId,
    /// Lower runs first.
    pub position: i64,
    /// Was running before a restart: goes before other waiting downloads.
    pub first: bool,
    pub activity: Activity,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Plan {
    pub start: Vec<DownloadId>,
    pub stop: Vec<DownloadId>,
}

/// Stops come from the back of each line; free slots fill in turn across open queues, so one
/// long queue cannot starve the others; a stopping download keeps its slot until it stops.
#[must_use]
pub fn plan(queues: &[QueueView], downloads: &[DownloadView], global_max: u32) -> Plan {
    let mut out = Plan::default();
    let mut held: HashMap<QueueId, u32> = HashMap::new();
    let mut running_total = 0u32;

    let mut running: Vec<&DownloadView> = downloads
        .iter()
        .filter(|d| d.activity != Activity::Waiting)
        .collect();
    // Stopping ones claim their slots before anyone else; then front of the line
    // first, so stops come from the back.
    running.sort_by_key(|d| (d.activity != Activity::Stopping, d.position));
    for d in running {
        let queue = queues.iter().find(|q| q.id == d.queue);
        let queue_room =
            queue.is_some_and(|q| q.open && has_room(q, held.get(&q.id).copied().unwrap_or(0)));
        let over = d.activity == Activity::Running && (!queue_room || running_total >= global_max);
        if over {
            out.stop.push(d.id);
            continue;
        }
        *held.entry(d.queue).or_default() += 1;
        running_total += 1;
    }

    let mut waiting: HashMap<QueueId, Vec<&DownloadView>> = HashMap::new();
    for d in downloads.iter().filter(|d| d.activity == Activity::Waiting) {
        waiting.entry(d.queue).or_default().push(d);
    }
    for line in waiting.values_mut() {
        // Reversed, so the next to start is popped from the end.
        line.sort_by_key(|d| std::cmp::Reverse((!d.first, d.position)));
    }

    loop {
        let mut started_any = false;
        for q in queues.iter().filter(|q| q.open) {
            if running_total >= global_max {
                return out;
            }
            let count = held.entry(q.id).or_default();
            if !has_room(q, *count) {
                continue;
            }
            if let Some(next) = waiting.get_mut(&q.id).and_then(Vec::pop) {
                out.start.push(next.id);
                *count += 1;
                running_total += 1;
                started_any = true;
            }
        }
        if !started_any {
            return out;
        }
    }
}

fn has_room(queue: &QueueView, held: u32) -> bool {
    queue.max_concurrent.is_none_or(|max| held < max)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn queue(id: i64, max: u32) -> QueueView {
        QueueView {
            id: QueueId(id),
            max_concurrent: Some(max),
            open: true,
        }
    }

    fn dl(id: i64, queue: i64, activity: Activity) -> DownloadView {
        DownloadView {
            id: DownloadId(id),
            queue: QueueId(queue),
            position: id,
            first: false,
            activity,
        }
    }

    fn ids(list: &[DownloadId]) -> Vec<i64> {
        list.iter().map(|d| d.0).collect()
    }

    #[test]
    fn fills_each_queue_up_to_its_limit() {
        let queues = [queue(1, 2)];
        let downloads: Vec<_> = (1..=5).map(|i| dl(i, 1, Activity::Waiting)).collect();
        let plan = plan(&queues, &downloads, 10);
        assert_eq!(ids(&plan.start), vec![1, 2]);
        assert_eq!(plan.stop, [] as [wirecatch_protocol::DownloadId; 0]);
    }

    #[test]
    fn a_queue_without_its_own_limit_runs_up_to_the_apps() {
        let queues = [QueueView {
            max_concurrent: None,
            ..queue(1, 0)
        }];
        let downloads: Vec<_> = (1..=5).map(|i| dl(i, 1, Activity::Waiting)).collect();
        let plan = plan(&queues, &downloads, 3);
        assert_eq!(ids(&plan.start), vec![1, 2, 3]);
    }

    #[test]
    fn global_limit_is_shared_in_turn_across_queues() {
        let queues = [queue(1, 3), queue(2, 3)];
        let downloads: Vec<_> = (1..=3)
            .map(|i| dl(i, 1, Activity::Waiting))
            .chain((4..=6).map(|i| dl(i, 2, Activity::Waiting)))
            .collect();
        let plan = plan(&queues, &downloads, 3);
        assert_eq!(ids(&plan.start), vec![1, 4, 2]);
    }

    #[test]
    fn running_and_stopping_downloads_hold_their_slots() {
        let queues = [queue(1, 2)];
        let downloads = [
            dl(1, 1, Activity::Stopping),
            dl(2, 1, Activity::Running),
            dl(3, 1, Activity::Waiting),
        ];
        assert_eq!(plan(&queues, &downloads, 10), Plan::default());
    }

    #[test]
    fn closed_queue_stops_its_downloads() {
        let mut q = queue(1, 2);
        q.open = false;
        let downloads = [
            dl(1, 1, Activity::Running),
            dl(2, 1, Activity::Stopping),
            dl(3, 1, Activity::Waiting),
        ];
        let plan = plan(&[q], &downloads, 10);
        assert_eq!(ids(&plan.stop), vec![1]);
        assert_eq!(plan.start, [] as [wirecatch_protocol::DownloadId; 0]);
    }

    #[test]
    fn lowered_limits_stop_the_ones_furthest_back() {
        let downloads: Vec<_> = (1..=4).map(|i| dl(i, 1, Activity::Running)).collect();
        assert_eq!(ids(&plan(&[queue(1, 2)], &downloads, 10).stop), vec![3, 4]);
        assert_eq!(
            ids(&plan(&[queue(1, 10)], &downloads, 1).stop),
            vec![2, 3, 4]
        );
    }

    #[test]
    fn downloads_resumed_after_a_restart_go_first() {
        let mut late = dl(9, 1, Activity::Waiting);
        late.first = true;
        let downloads = [dl(1, 1, Activity::Waiting), late];
        assert_eq!(ids(&plan(&[queue(1, 1)], &downloads, 10).start), vec![9]);
    }

    #[test]
    fn downloads_of_an_unknown_queue_never_run() {
        let downloads = [dl(1, 7, Activity::Waiting), dl(2, 7, Activity::Running)];
        let plan = plan(&[queue(1, 1)], &downloads, 10);
        assert_eq!(plan.start, [] as [wirecatch_protocol::DownloadId; 0]);
        assert_eq!(ids(&plan.stop), vec![2]);
    }
}
