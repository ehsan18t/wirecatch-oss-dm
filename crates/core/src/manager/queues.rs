//! Creating, editing and deleting queues.

use wirecatch_protocol::QueueId;
use wirecatch_protocol::app::{Queue, UiEvent};
use wirecatch_protocol::wire::{Event, QueueSummary};

use super::Manager;
use super::state::{QueueSlot, State, Stop};
use crate::error::{CoreError, Result};
use crate::settings;

/// Downloads at once in a newly created queue.
const NEW_QUEUE_MAX_CONCURRENT: u32 = 2;

impl Manager {
    /// Every queue, in sidebar order. The default queue is always there.
    pub fn list_queues(&self) -> Vec<Queue> {
        self.lock().queue_list()
    }

    /// Queue names and ids, as socket clients see them.
    pub(crate) fn queue_summaries(&self) -> Vec<QueueSummary> {
        self.lock().queue_summaries()
    }

    /// Adds a queue with no schedule and no speed limit.
    pub fn create_queue(&self, name: &str) -> Result<Queue> {
        let mut state = self.lock_open()?;
        settings::check_queue_count(state.queues.len() + 1)?;
        let queue = settings::validate_queue(Queue {
            id: QueueId(0),
            name: name.to_string(),
            max_concurrent: NEW_QUEUE_MAX_CONCURRENT,
            speed_limit: None,
            schedule: None,
            is_default: false,
        })?;
        ensure_name_free(&state, &queue.name, None)?;
        let queue = Queue {
            id: state.ids.queue(),
            ..queue
        };
        state.queues.push(QueueSlot::new(queue.clone()));
        self.queues_changed(&mut state);
        Ok(queue)
    }

    /// Changes a queue's name, limits or schedule. New limits apply to running
    /// downloads at once. Whether a queue is the default cannot be changed.
    pub fn update_queue(&self, queue: Queue) -> Result<Queue> {
        let queue = settings::validate_queue(queue)?;
        let mut state = self.lock_open()?;
        ensure_name_free(&state, &queue.name, Some(queue.id))?;
        let slot = state.slot_mut(queue.id)?;
        let updated = Queue {
            is_default: slot.queue.is_default,
            ..queue
        };
        apply_to_slot(slot, updated.clone());
        self.queues_changed(&mut state);
        Ok(updated)
    }

    /// Deletes a queue. Its downloads move to the end of the default queue.
    pub fn delete_queue(&self, id: QueueId) -> Result<()> {
        let mut state = self.lock_open()?;
        if state.slot(id)?.queue.is_default {
            return Err(CoreError::DefaultQueue);
        }
        state.queues.retain(|s| s.queue.id != id);
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
                entry.position = position;
                self.stop_run(entry, Stop::Requeue);
            }
            self.save(&mut state, download);
        }
        if state.settings.extension.default_queue == id {
            let mut settings = state.settings.clone();
            settings.extension.default_queue = fallback;
            self.apply_settings(&mut state, settings);
        }
        self.queues_changed(&mut state);
        Ok(())
    }

    /// Saves the queues, tells everyone, and re-plans with the new limits.
    pub(super) fn queues_changed(&self, state: &mut State) {
        self.inner.store.save_queues(state.queue_list());
        self.emit(UiEvent::QueuesChanged(state.queue_list()));
        self.emit_wire(Event::QueuesChanged(state.queue_summaries()));
        self.inner.queues_changed.notify_one();
        self.reconcile(state);
    }
}

/// Replaces a slot's queue, applying a new speed limit live and restarting the
/// schedule's bookkeeping if the schedule changed.
pub(super) fn apply_to_slot(slot: &mut QueueSlot, queue: Queue) {
    if slot.queue.schedule != queue.schedule {
        slot.gate.reset();
    }
    if slot.queue.speed_limit != queue.speed_limit {
        slot.limiter.set_limit(queue.speed_limit);
    }
    slot.queue = queue;
}

fn ensure_name_free(state: &State, name: &str, except: Option<QueueId>) -> Result<()> {
    if state.queue_name_taken(name, except) {
        return Err(CoreError::Invalid(format!(
            "A queue named \"{name}\" already exists. Choose another name."
        )));
    }
    Ok(())
}
