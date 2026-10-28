//! Creating, editing, ordering and deleting queues.

use std::collections::BTreeSet;

use wirecatch_protocol::QueueId;
use wirecatch_protocol::app::{Queue, QueueSettings, QueueStatus, UiEvent};
use wirecatch_protocol::claims::{LinkClaims, LinkTrial, QueueReason};
use wirecatch_protocol::wire::{Event, QueueSummary};

use super::Manager;
use super::downloads::parse_url;
use super::state::{QueueSlot, State, Stop};
use crate::error::{CoreError, Result};

impl Manager {
    /// Every queue, in sidebar order. The default queue is always there.
    #[must_use]
    pub fn list_queues(&self) -> Vec<Queue> {
        self.lock().queue_list()
    }

    /// Queue names and ids, as socket clients see them.
    #[must_use]
    pub fn queue_summaries(&self) -> Vec<QueueSummary> {
        self.lock().queue_summaries()
    }

    /// Where `trial`'s link would go by the claims, saved or as one queue's editor has them (D72.5, D72.7).
    ///
    /// # Errors
    ///
    /// [`CoreError::InvalidUrl`] for a link that is not http, https or FTP, and [`CoreError::Closed`] once the manager has shut down.
    pub fn try_link(&self, trial: &LinkTrial) -> Result<LinkClaims> {
        let url = parse_url(&trial.url)?;
        Ok(self.lock_open()?.try_link(&url, trial.draft.as_ref()))
    }

    /// Adds a running queue, last in the sidebar.
    pub fn create_queue(&self, settings: QueueSettings) -> Result<Queue> {
        let mut state = self.lock_open()?;
        wirecatch_settings::check_queue_count(state.queues.len() + 1)?;
        let settings = wirecatch_settings::validate_queue(settings)?;
        ensure_name_free(&state, &settings.name, None)?;
        let queue = Queue {
            id: state.ids.queue(),
            is_default: false,
            settings,
            status: QueueStatus::default(),
        };
        self.inner.store.save_last_ids(state.ids.last());
        state.queues.push(QueueSlot::new(queue.clone()));
        self.queues_changed(&mut state);
        Ok(queue)
    }

    /// Replaces queue `id`'s settings; new limits apply to running downloads at once.
    pub fn update_queue(&self, id: QueueId, settings: QueueSettings) -> Result<()> {
        let settings = wirecatch_settings::validate_queue(settings)?;
        let mut state = self.lock_open()?;
        ensure_name_free(&state, &settings.name, Some(id))?;
        apply_to_slot(state.slot_mut(id)?, settings);
        self.queues_changed(&mut state);
        Ok(())
    }

    /// Pauses or resumes a queue. A paused queue starts nothing, including
    /// downloads added later, and its running downloads stop and wait. Resuming it
    /// lets those continue, but not downloads the user paused one by one. Pausing
    /// also ends a "start now" run.
    ///
    /// # Errors
    ///
    /// [`CoreError::QueueNotFound`] when there is no queue `id`, and
    /// [`CoreError::Closed`] once the manager has shut down.
    pub fn set_queue_paused(&self, id: QueueId, paused: bool) -> Result<()> {
        let mut state = self.lock_open()?;
        let status = &mut state.slot_mut(id)?.queue.status;
        status.paused = paused;
        if paused {
            status.running_outside_schedule = false;
        }
        self.queues_changed(&mut state);
        Ok(())
    }

    /// Runs a queue now, ignoring its schedule, until it has nothing left to do or
    /// is paused; then its schedule applies again. Starting a paused queue now also
    /// resumes it.
    ///
    /// # Errors
    ///
    /// [`CoreError::QueueNotFound`] when there is no queue `id`, and
    /// [`CoreError::Closed`] once the manager has shut down.
    pub fn start_queue_now(&self, id: QueueId) -> Result<()> {
        let mut state = self.lock_open()?;
        let status = &mut state.slot_mut(id)?.queue.status;
        status.paused = false;
        status.running_outside_schedule = true;
        self.queues_changed(&mut state);
        Ok(())
    }

    /// Deletes a queue. Its downloads move to the end of the default queue, and
    /// captures waiting for confirmation that were meant for it go there too.
    ///
    /// # Errors
    ///
    /// [`CoreError::DefaultQueue`] for the default queue, [`CoreError::QueueNotFound`]
    /// when there is no queue `id`, and [`CoreError::Closed`] once the manager has shut
    /// down.
    pub fn delete_queue(&self, id: QueueId) -> Result<()> {
        let mut state = self.lock_open()?;
        if state.slot(id)?.queue.is_default {
            return Err(CoreError::DefaultQueue);
        }
        state.queues.retain(|s| s.queue.id != id);
        state.batches.forget_queue(id);
        let fallback = state.default_queue();
        let moved: Vec<_> = state
            .downloads
            .values()
            .filter(|e| e.row.queue == id)
            .map(|e| e.row.id)
            .collect();
        for download in moved {
            let position = state.ids.position();
            if let Some(entry) = state.downloads.get_mut(&download) {
                entry.row.queue = fallback;
                entry.row.queue_reason = QueueReason::DefaultQueue;
                entry.position = position;
                self.stop_run(entry, Stop::Requeue);
            }
            self.save(&mut state, download);
        }
        // Captures waiting for the user's answer go with them.
        for waiting in state.pending.values_mut() {
            if waiting.capture.queue == id {
                waiting.capture.queue = fallback;
                waiting.capture.queue_reason = QueueReason::DefaultQueue;
            }
        }
        if state.settings.extension.default_queue == id {
            let mut settings = state.settings.clone();
            settings.extension.default_queue = fallback;
            self.apply_settings(&mut state, settings);
        }
        self.queues_changed(&mut state);
        Ok(())
    }

    /// Puts the queues in `order`, the sidebar's, which also settles ties between claims (D72.9); `order` names every queue once, the default queue first.
    pub fn set_queue_order(&self, order: &[QueueId]) -> Result<()> {
        let mut state = self.lock_open()?;
        if order.first() != Some(&state.default_queue()) {
            return Err(CoreError::Invalid(
                "The default queue stays first in the list.".into(),
            ));
        }
        let current: BTreeSet<QueueId> = state.queues.iter().map(|s| s.queue.id).collect();
        let named: BTreeSet<QueueId> = order.iter().copied().collect();
        if named != current || named.len() != order.len() {
            return Err(CoreError::Invalid(
                "The queues changed while you were moving them. Try again.".into(),
            ));
        }
        let mut slots = std::mem::take(&mut state.queues);
        slots.sort_by_key(|s| order.iter().position(|id| *id == s.queue.id));
        state.queues = slots;
        self.publish_queues(&state);
        Ok(())
    }

    /// Saves the queues, tells everyone, and re-plans with the new limits.
    pub(super) fn queues_changed(&self, state: &mut State) {
        self.publish_queues(state);
        self.inner.queues_changed.notify_one();
        self.reconcile(state);
    }

    /// Saves the queues and tells the UI and every socket client.
    pub(super) fn publish_queues(&self, state: &State) {
        self.inner.store.save_queues(state.queue_list());
        self.emit(UiEvent::QueuesChanged(state.queue_list()));
        self.emit_wire(Event::QueuesChanged(state.queue_summaries()));
    }
}

/// A changed schedule restarts the gate's bookkeeping.
pub(super) fn apply_to_slot(slot: &mut QueueSlot, settings: QueueSettings) {
    let old = &slot.queue.settings;
    if old.schedule != settings.schedule {
        slot.gate.reset();
    }
    if old.speed_limit != settings.speed_limit {
        slot.limiter.set_limit(settings.speed_limit.own());
    }
    slot.queue.settings = settings;
}

fn ensure_name_free(state: &State, name: &str, except: Option<QueueId>) -> Result<()> {
    if state.queue_name_taken(name, except) {
        return Err(CoreError::Invalid(format!(
            "A queue named \"{name}\" already exists. Choose another name."
        )));
    }
    Ok(())
}
