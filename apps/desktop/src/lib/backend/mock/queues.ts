// What the mock core does to queues: create, edit, pause, start now and delete them.

import type { DownloadSummary, Queue, QueueId, QueueSettings } from '@wirecatch/protocol';
import { scheduleAllows } from '../../format';
import { BackendError } from '../types';
import { QUEUE_RUNNING } from './data';
import type { MockState } from './state';

const NO_QUEUE_NAME = 'Give the queue a name.';

export function listQueues(state: MockState): Queue[] {
  return state.queues.map((q) => structuredClone(q));
}

function trimmedName(settings: QueueSettings): QueueSettings {
  const name = settings.name.trim();
  if (!name) throw new BackendError(NO_QUEUE_NAME);
  return { ...structuredClone(settings), name };
}

export function createQueue(state: MockState, settings: QueueSettings): Queue {
  const q: Queue = {
    id: state.newQueueId(),
    is_default: false,
    settings: trimmedName(settings),
    status: { ...QUEUE_RUNNING },
  };
  state.queues.push(q);
  state.queuesChanged();
  return structuredClone(q);
}

export function updateQueue(state: MockState, id: QueueId, settings: QueueSettings): void {
  const q = state.queues.find((x) => x.id === id);
  if (!q) throw new BackendError('That queue no longer exists.');
  q.settings = trimmedName(settings);
  state.queuesChanged();
}

export function setQueuePaused(state: MockState, id: QueueId, paused: boolean): void {
  const status = state.queue(id).status;
  status.paused = paused;
  if (paused) status.running_outside_schedule = false;
  state.queuesChanged();
}

export function startQueueNow(state: MockState, id: QueueId): void {
  const q = state.queue(id);
  q.status.paused = false;
  if (!scheduleAllows(q.settings.schedule, new Date(state.now())))
    q.status.running_outside_schedule = true;
  state.queuesChanged();
}

export function deleteQueue(state: MockState, id: QueueId): void {
  const q = state.queue(id);
  if (q.is_default)
    throw new BackendError('The default queue cannot be deleted. Rename it instead.');
  const fallback = state.defaultQueueId();
  state.queues = state.queues.filter((x) => x.id !== id);
  const moved: DownloadSummary[] = [];
  for (const row of state.rows.values()) {
    if (row.queue === id) {
      row.queue = fallback;
      moved.push(row);
    }
  }
  if (state.settings.extension.default_queue === id) {
    state.settings.extension.default_queue = fallback;
    state.settingsChanged();
  }
  state.queuesChanged();
  state.changed(moved);
}
