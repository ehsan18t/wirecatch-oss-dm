// What the mock core does to queues: create, edit, pause, start now and delete them.

import type {
  DownloadSummary,
  LinkClaims,
  LinkTrial,
  Queue,
  QueueId,
  QueueSettings,
  QueueTakes,
  TriedClaim,
} from '@wirecatch/protocol';
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

/** Puts the queues in `order`, which must name each once with the default queue first, as the core asks. */
export function setQueueOrder(state: MockState, order: QueueId[]): void {
  if (order[0] !== state.queues.find((q) => q.is_default)?.id)
    throw new BackendError('The default queue stays first in the list.');
  const named = new Set(order);
  if (
    named.size !== order.length ||
    named.size !== state.queues.length ||
    state.queues.some((q) => !named.has(q.id))
  )
    throw new BackendError('The queues changed while you were moving them. Try again.');
  state.queues.sort((a, b) => order.indexOf(a.id) - order.indexOf(b.id));
  state.queuesChanged();
}

/** Try a link in the mock, by site claims alone: a name covers itself and the names under it; drafted regular expressions are checked by JavaScript's reader. The app matches every kind and pattern in `wirecatch-queues`; this stands in for it so the windows can be built against sample data. */
export function tryLink(state: MockState, trial: LinkTrial): LinkClaims {
  let host: string;
  try {
    host = new URL(trial.url).hostname;
  } catch {
    throw new BackendError(
      `"${trial.url}" is not a valid link. Check that it starts with http://, https://, ftp:// or ftps://.`,
      'invalid_url',
    );
  }
  const { draft } = trial;
  if (draft?.takes.kind === 'auto' && draft.takes.file_name_syntax === 'regex') {
    const unreadable = draft.takes.file_names.find((name) => !readsAsRegex(name));
    if (unreadable !== undefined)
      throw new BackendError(`"${unreadable}" is not a regular expression Wirecatch can use.`);
  }
  const queues: { id: QueueId | null; takes: QueueTakes }[] = state.queues.map((q) => ({
    id: q.id,
    takes: draft?.queue === q.id ? draft.takes : q.settings.takes,
  }));
  if (draft?.queue === null) queues.push({ id: null, takes: draft.takes });
  const claimOf = (enabled: boolean): TriedClaim | null => {
    for (const { id, takes } of queues) {
      if (takes.kind !== 'auto' || takes.enabled !== enabled) continue;
      const site = takes.sites.find((s) => host === s || host.endsWith(`.${s}`));
      if (site !== undefined) return { queue: id, claim: { kind: 'site', claim: site } };
    }
    return null;
  };
  return { taken: claimOf(true), switched_off: claimOf(false) };
}

function readsAsRegex(pattern: string): boolean {
  try {
    return new RegExp(pattern).source !== '';
  } catch {
    return false;
  }
}
