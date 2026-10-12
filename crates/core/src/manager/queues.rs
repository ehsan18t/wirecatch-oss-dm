//! Creating, editing and deleting queues.

use wirecatch_protocol::QueueId;
use wirecatch_protocol::app::{NEW_QUEUE_MAX_CONCURRENT, Queue, UiEvent};
use wirecatch_protocol::wire::{Event, QueueSummary};

use super::Manager;
use super::state::{QueueSlot, State, Stop};
use crate::error::{CoreError, Result};
use crate::settings;

impl Manager {
    /// Every queue, in sidebar order. The default queue is always there.
    #[must_use]
    pub fn list_queues(&self) -> Vec<Queue> {
        self.lock().queue_list()
    }

    /// Queue names and ids, as socket clients see them.
    pub(crate) fn queue_summaries(&self) -> Vec<QueueSummary> {
        self.lock().queue_summaries()
    }

    /// Adds a queue with no schedule and no speed limit.
    ///
    /// # Errors
    ///
    /// [`CoreError::Invalid`] when the name is empty, too long or taken, or there are
    /// already as many queues as allowed; and [`CoreError::Closed`] once the manager
    /// has shut down.
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
            paused: false,
            running_outside_schedule: false,
        })?;
        ensure_name_free(&state, &queue.name, None)?;
        let queue = Queue {
            id: state.ids.queue(),
            ..queue
        };
        self.inner.store.save_last_ids(state.ids.last());
        state.queues.push(QueueSlot::new(queue.clone()));
        self.queues_changed(&mut state);
        Ok(queue)
    }

    /// Changes a queue's name, limits or schedule. New limits apply to running
    /// downloads at once. Whether a queue is the default, paused or started now is
    /// not changed here (see [`set_queue_paused`](Self::set_queue_paused) and
    /// [`start_queue_now`](Self::start_queue_now)). The stored queue arrives as
    /// `QueuesChanged`.
    ///
    /// # Errors
    ///
    /// [`CoreError::Invalid`] for a name or limit that cannot be used,
    /// [`CoreError::QueueNotFound`] when there is no such queue, and
    /// [`CoreError::Closed`] once the manager has shut down.
    pub fn update_queue(&self, queue: Queue) -> Result<()> {
        let queue = settings::validate_queue(queue)?;
        let mut state = self.lock_open()?;
        ensure_name_free(&state, &queue.name, Some(queue.id))?;
        apply_to_slot(state.slot_mut(queue.id)?, queue);
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
        let queue = &mut state.slot_mut(id)?.queue;
        queue.paused = paused;
        if paused {
            queue.running_outside_schedule = false;
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
        let queue = &mut state.slot_mut(id)?.queue;
        queue.paused = false;
        queue.running_outside_schedule = true;
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
        // Captures waiting for the user's answer go with them.
        for waiting in state.pending.values_mut() {
            if waiting.capture.queue == id {
                waiting.capture.queue = fallback;
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

/// Replaces a slot's settings (name, limits, schedule) with `queue`'s, applying a
/// new speed limit live and restarting the schedule's bookkeeping if the schedule
/// changed. Whether the queue is the default, paused or started now is kept.
pub(super) fn apply_to_slot(slot: &mut QueueSlot, queue: Queue) {
    if slot.queue.schedule != queue.schedule {
        slot.gate.reset();
    }
    if slot.queue.speed_limit != queue.speed_limit {
        slot.limiter.set_limit(queue.speed_limit);
    }
    slot.queue = Queue {
        id: slot.queue.id,
        is_default: slot.queue.is_default,
        paused: slot.queue.paused,
        running_outside_schedule: slot.queue.running_outside_schedule,
        ..queue
    };
}

fn ensure_name_free(state: &State, name: &str, except: Option<QueueId>) -> Result<()> {
    if state.queue_name_taken(name, except) {
        return Err(CoreError::Invalid(format!(
            "A queue named \"{name}\" already exists. Choose another name."
        )));
    }
    Ok(())
}
