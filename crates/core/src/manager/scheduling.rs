//! Deciding what runs: each queue's gate (paused, schedule, started now), the
//! planner, and why every waiting download is waiting.

use std::collections::{HashMap, HashSet};

use wirecatch_engine::numeric::saturating_cast;
use wirecatch_protocol::app::{DownloadStatus, TimeOfDay, WaitReason};
use wirecatch_protocol::{DownloadId, QueueId};

use super::Manager;
use super::state::{Entry, State, Stop};
use wirecatch_queues::plan::{self, Activity, DownloadView, QueueView};

/// Why a queue may not start downloads right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Closed {
    Paused,
    /// Outside its schedule; the time of day it starts next.
    Schedule(Option<TimeOfDay>),
}

/// One queue as this round of decisions sees it.
#[derive(Debug, Clone, Copy)]
struct GateView {
    id: QueueId,
    /// `None`: no limit of the queue's own.
    max_concurrent: Option<u32>,
    closed: Option<Closed>,
}

impl Manager {
    /// Starts and stops downloads so every queue, schedule and limit is honoured,
    /// then brings every waiting download's reason up to date.
    pub(super) fn reconcile(&self, state: &mut State) {
        if state.closed {
            return;
        }
        state.prune_finished();
        let gates = self.evaluate_gates(state);
        let queues: Vec<QueueView> = gates
            .iter()
            .map(|g| QueueView {
                id: g.id,
                max_concurrent: g.max_concurrent,
                open: g.closed.is_none(),
            })
            .collect();
        let downloads: Vec<DownloadView> = state.unfinished().filter_map(view_of).collect();
        let plan = plan::plan(&queues, &downloads, state.settings.max_concurrent_downloads);
        for id in plan.stop {
            if let Some(entry) = state.downloads.get_mut(&id) {
                self.stop_run(entry, Stop::Requeue);
            }
        }
        for id in plan.start {
            self.start_run(state, id);
        }
        self.update_wait_reasons(state, &gates);
    }

    /// Works out whether each queue may start downloads, and ends a "start now"
    /// run whose queue has emptied or been paused.
    fn evaluate_gates(&self, state: &mut State) -> Vec<GateView> {
        let now = self.inner.clock.local_now();
        let since = std::mem::replace(&mut state.last_checked, now);
        let busy: HashSet<QueueId> = state
            .unfinished()
            .filter_map(view_of)
            .map(|d| d.queue)
            .collect();
        let mut flags_changed = false;
        let gates = state
            .queues
            .iter_mut()
            .map(|slot| {
                let id = slot.queue.id;
                let settings = &slot.queue.settings;
                let status = &mut slot.queue.status;
                let has_work = busy.contains(&id);
                // Always evaluated, so a start time crossed meanwhile is not missed.
                let in_schedule =
                    slot.gate
                        .is_open(settings.schedule.as_ref(), since, now, has_work);
                if status.running_outside_schedule && (!has_work || status.paused) {
                    status.running_outside_schedule = false;
                    flags_changed = true;
                }
                let closed = if status.paused {
                    Some(Closed::Paused)
                } else if status.running_outside_schedule || in_schedule {
                    None
                } else {
                    Some(Closed::Schedule(
                        settings.schedule.as_ref().map(|s| s.start),
                    ))
                };
                GateView {
                    id,
                    max_concurrent: settings.max_concurrent.own(),
                    closed,
                }
            })
            .collect();
        if flags_changed {
            self.publish_queues(state);
        }
        gates
    }

    /// Sets the reason on every waiting download and clears it on the rest,
    /// queueing a row event for each one that changed.
    fn update_wait_reasons(&self, state: &mut State, gates: &[GateView]) {
        let limit = state.settings.max_concurrent_downloads;
        let running_total = saturating_cast::<u32, _>(state.running.len());
        let mut running: HashMap<QueueId, u32> = HashMap::new();
        for id in &state.running {
            if let Some(entry) = state.downloads.get(id) {
                *running.entry(entry.row.queue).or_default() += 1;
            }
        }
        let mut lines: HashMap<QueueId, Vec<&Entry>> = HashMap::new();
        for entry in state.unfinished().filter(|e| is_waiting(e)) {
            lines.entry(entry.row.queue).or_default().push(entry);
        }

        let mut reasons: HashMap<DownloadId, WaitReason> = HashMap::new();
        for (queue, mut line) in lines {
            line.sort_by_key(|e| (!e.resume_first, e.position));
            let gate = gates.iter().find(|g| g.id == queue);
            let in_queue = running.get(&queue).copied().unwrap_or(0);
            for (place, entry) in line.into_iter().enumerate() {
                let reason = wait_reason(gate, place, in_queue, running_total, limit);
                reasons.insert(entry.row.id, reason);
            }
        }

        let mut changed = Vec::new();
        for id in &state.unfinished {
            let Some(entry) = state.downloads.get_mut(id) else {
                continue;
            };
            let reason = reasons.remove(&entry.row.id);
            // A running video waiting to try again keeps the time of its next try;
            // the run sets and clears that one itself.
            if reason.is_none()
                && entry.run.is_some()
                && matches!(entry.row.waiting, Some(WaitReason::ServerRefused { .. }))
            {
                continue;
            }
            if entry.row.waiting != reason {
                entry.row.waiting = reason;
                changed.push(entry.row.id);
            }
        }
        if !changed.is_empty() {
            for id in changed {
                state.touch(id);
            }
            self.inner.rows_changed.notify_one();
        }
    }
}

/// The single most useful reason a download waits, by priority: its queue is
/// paused, outside its schedule, the app runs as many downloads as allowed (for
/// the first in line), its queue runs as many as allowed, or others are ahead.
fn wait_reason(
    gate: Option<&GateView>,
    place: usize,
    in_queue: u32,
    running_total: u32,
    global_limit: u32,
) -> WaitReason {
    let in_line = WaitReason::InLine {
        position: saturating_cast(place + 1),
    };
    let Some(gate) = gate else {
        return in_line;
    };
    let queue_limit = gate.max_concurrent.filter(|&max| in_queue >= max);
    match gate.closed {
        Some(Closed::Paused) => WaitReason::QueuePaused,
        Some(Closed::Schedule(starts_at)) => WaitReason::QueueSchedule { starts_at },
        None if running_total >= global_limit && place == 0 => WaitReason::GlobalLimit {
            running: running_total,
            limit: global_limit,
        },
        None if let Some(limit) = queue_limit => WaitReason::QueueFull {
            running: in_queue,
            limit,
        },
        None => in_line,
    }
}

/// Queued and not running, moving or being removed.
fn is_waiting(entry: &Entry) -> bool {
    entry.run.is_none() && entry.hold.is_free() && entry.row.status == DownloadStatus::Queued
}

/// How the planner sees a download; `None` for ones that neither run nor wait.
fn view_of(entry: &Entry) -> Option<DownloadView> {
    let activity = match &entry.run {
        Some(run) if run.stop.is_some() => Activity::Stopping,
        Some(_) => Activity::Running,
        None if is_waiting(entry) => Activity::Waiting,
        None => return None,
    };
    Some(DownloadView {
        id: entry.row.id,
        queue: entry.row.queue,
        position: entry.position,
        first: entry.resume_first,
        activity,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gate(closed: Option<Closed>, max: u32) -> GateView {
        GateView {
            id: QueueId(1),
            max_concurrent: Some(max),
            closed,
        }
    }

    #[test]
    fn reasons_follow_their_priority() {
        let at_two = Some(TimeOfDay { hour: 2, minute: 0 });
        let open = gate(None, 2);
        let reason =
            |g: &GateView, place, in_queue, total| wait_reason(Some(g), place, in_queue, total, 3);
        assert_eq!(
            reason(&gate(Some(Closed::Paused), 2), 0, 2, 3),
            WaitReason::QueuePaused
        );
        assert_eq!(
            reason(&gate(Some(Closed::Schedule(at_two)), 2), 0, 0, 3),
            WaitReason::QueueSchedule { starts_at: at_two }
        );
        // The app is full: the first in line waits for that, the rest for it.
        assert_eq!(
            reason(&open, 0, 1, 3),
            WaitReason::GlobalLimit {
                running: 3,
                limit: 3
            }
        );
        assert_eq!(reason(&open, 1, 1, 3), WaitReason::InLine { position: 2 });
        assert_eq!(
            reason(&open, 1, 2, 3),
            WaitReason::QueueFull {
                running: 2,
                limit: 2
            }
        );
        assert_eq!(
            reason(&open, 0, 2, 2),
            WaitReason::QueueFull {
                running: 2,
                limit: 2
            }
        );
        assert_eq!(reason(&open, 2, 0, 0), WaitReason::InLine { position: 3 });
    }
}
