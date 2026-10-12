// What the mock core does to queues: create, edit, pause, start now and delete them.

import {
  NEW_QUEUE_MAX_CONCURRENT,
  type DownloadSummary,
  type Queue,
  type QueueId,
} from '@wirecatch/protocol';
import { scheduleAllows } from '../../format';
import { BackendError } from '../types';
import type { MockState } from './state';

const NO_QUEUE_NAME = 'Give the queue a name.';

export function listQueues(state: MockState): Queue[] {
  return state.queues.map((q) => structuredClone(q));
}

export function createQueue(state: MockState, name: string): Queue {
  const trimmed = name.trim();
  if (!trimmed) throw new BackendError(NO_QUEUE_NAME);
  const q: Queue = {
    id: state.nextQueueId++,
    name: trimmed,
    max_concurrent: NEW_QUEUE_MAX_CONCURRENT,
    speed_limit: null,
    schedule: null,
    is_default: false,
    paused: false,
    running_outside_schedule: false,
  };
  state.queues.push(q);
  state.queuesChanged();
  return structuredClone(q);
}

export function updateQueue(state: MockState, queue: Queue): void {
  const i = state.queues.findIndex((q) => q.id === queue.id);
  if (i < 0) throw new BackendError('That queue no longer exists.');
  if (!queue.name.trim()) throw new BackendError(NO_QUEUE_NAME);
  const old = state.queues[i];
  // Pausing and "start now" have their own commands; an edit keeps the queue's current state.
  state.queues[i] = {
    ...structuredClone(queue),
    is_default: old?.is_default ?? false,
    paused: old?.paused ?? false,
    running_outside_schedule: old?.running_outside_schedule ?? false,
  };
  state.queuesChanged();
}

export function setQueuePaused(state: MockState, id: QueueId, paused: boolean): void {
  const q = state.queue(id);
  q.paused = paused;
  if (paused) q.running_outside_schedule = false;
  state.queuesChanged();
}

export function startQueueNow(state: MockState, id: QueueId): void {
  const q = state.queue(id);
  q.paused = false;
  if (!scheduleAllows(q.schedule, new Date(state.now()))) q.running_outside_schedule = true;
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
