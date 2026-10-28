// What the mock core does to downloads: add, pause, resume, cancel, retry, move and remove them.

import type {
  AddRequest,
  DownloadId,
  DownloadSummary,
  QueueId,
  QueueReason,
} from '@wirecatch/protocol';
import { fileType, isProgramName } from '../../filetypes';
import { hostOf } from '../../format';
import { BackendError } from '../types';
import { tryLink } from './queues';
import { RUNNING, stop, type MockState } from './state';

const NOT_A_LINK = 'That does not look like a link. Copy the full address, starting with https://.';

/** A link the core takes; one on the reserved `.invalid` domain stands for a link it refuses. */
function requireLink(url: string) {
  let host: string;
  try {
    host = new URL(url).hostname;
  } catch {
    throw new BackendError(NOT_A_LINK);
  }
  if (host.endsWith('.invalid')) throw new BackendError(NOT_A_LINK);
}

/** Puts `row` back in line and clears its error, then reports it. */
function requeue(state: MockState, row: DownloadSummary) {
  row.status = 'queued';
  row.error = null;
  state.changed([row]);
}

export function listDownloads(state: MockState): DownloadSummary[] {
  return state.order.flatMap((id) => {
    const r = state.rows.get(id);
    return r ? [{ ...r }] : [];
  });
}

export function addDownload(state: MockState, request: AddRequest): DownloadId {
  requireLink(request.url);
  const row = state.create(request.url, {
    ...placed(state, request),
    folder: request.folder,
    fileName: request.file_name,
    start: request.start,
  });
  state.changed([row]);
  return row.id;
}

/** The queue a download added in the app joins, and why: the one picked, else the claims', else the default queue (D72.3). */
function placed(
  state: MockState,
  request: AddRequest,
): { queue: QueueId | null; queue_reason: QueueReason } {
  if (request.queue !== null) return { queue: request.queue, queue_reason: { kind: 'picked' } };
  const { taken } = tryLink(state, { url: request.url, draft: null });
  const queue = taken?.queue ?? null;
  if (taken === null || queue === null)
    return { queue: null, queue_reason: { kind: 'default_queue' } };
  return { queue, queue_reason: { kind: 'claimed', claim: taken.claim } };
}

export function pause(state: MockState, id: DownloadId): void {
  const row = state.row(id);
  if (row.status === 'completed' || row.status === 'paused') return;
  row.status = 'paused';
  row.waiting = null;
  // A paused conversion starts again from the download when resumed.
  row.processing = null;
  stop(row);
  state.changed([row]);
}

export function resume(state: MockState, id: DownloadId): void {
  const row = state.row(id);
  if (row.status !== 'paused' && row.status !== 'failed') return;
  requeue(state, row);
}

export function cancel(state: MockState, id: DownloadId): void {
  const row = state.row(id);
  if (row.status === 'completed' || row.status === 'cancelled') return;
  row.status = 'cancelled';
  row.done = 0;
  row.error = null;
  stop(row);
  state.changed([row]);
}

export function remove(state: MockState, ids: DownloadId[]): void {
  const gone = new Set(ids.filter((id) => state.rows.has(id)));
  for (const id of gone) {
    state.rows.delete(id);
    state.sim.delete(id);
  }
  state.order = state.order.filter((id) => !gone.has(id));
  if (gone.size > 0) state.emit({ type: 'downloads_removed', data: [...gone] });
}

export function retry(state: MockState, id: DownloadId): void {
  const row = state.row(id);
  // A cancelled download lost its partial file, so it starts from zero.
  if (row.status === 'cancelled') row.done = 0;
  requeue(state, row);
}

export function refreshLink(state: MockState, id: DownloadId, url: string): void {
  requireLink(url);
  const row = state.row(id);
  row.url = url;
  requeue(state, row);
}

export function keepOriginal(state: MockState, id: DownloadId): void {
  const row = state.row(id);
  if (row.status !== 'failed' || row.error?.fix !== 'keep_original') {
    throw new BackendError('This download is not waiting to be kept as it was downloaded.');
  }
  const sim = state.sim.get(id);
  if (sim) sim.step = null;
  requeue(state, row);
}

export function moveToQueue(state: MockState, ids: DownloadId[], queue: QueueId): void {
  state.queue(queue);
  const rows = ids.flatMap((id) => {
    const r = state.rows.get(id);
    if (!r || r.queue === queue) return [];
    r.queue = queue;
    r.queue_reason = { kind: 'picked' };
    return [r];
  });
  state.changed(rows);
}

export function reorder(state: MockState, queue: QueueId, idsInOrder: DownloadId[]): void {
  const inQueue = state.order.filter((id) => state.rows.get(id)?.queue === queue);
  const wanted = idsInOrder.filter((id) => inQueue.includes(id));
  const rest = inQueue.filter((id) => !wanted.includes(id));
  const next = [...wanted, ...rest];
  let i = 0;
  state.order = state.order.map((id) =>
    state.rows.get(id)?.queue === queue ? (next[i++] ?? id) : id,
  );
}

export function pauseAll(state: MockState): void {
  const rows: DownloadSummary[] = [];
  for (const row of state.rows.values()) {
    if (row.status === 'queued' || RUNNING.has(row.status)) {
      row.status = 'paused';
      stop(row);
      rows.push(row);
    }
  }
  state.changed(rows);
}

export function resumeAll(state: MockState): void {
  const rows: DownloadSummary[] = [];
  for (const row of state.rows.values()) {
    if (row.status === 'paused') {
      row.status = 'queued';
      rows.push(row);
    }
  }
  state.changed(rows);
}

/**
 * Like the shell: a file opens once it is finished, and a program only when the person agrees to
 * the warning `ask` shows, with the shell's wording.
 */
export function mayOpen(
  state: MockState,
  id: DownloadId,
  ask: (warning: string) => boolean,
): boolean {
  const row = state.row(id);
  if (row.status !== 'completed') {
    throw new BackendError('The file is not finished yet. It opens once the download completes.');
  }
  if (!isProgramName(row.file_name)) return true;
  const type = fileType(row.file_name);
  const what =
    type === null
      ? `${row.file_name} has no file type, so it may run code.`
      : `This is a .${type} file, a type that can run code.`;
  return ask(
    `${what} Only open it if you trust ${hostOf(row.url)}. Code can change your computer, and a file from an unknown site can contain anything. To look at it first, choose Cancel and use Show in folder.`,
  );
}

export function setDownloadFolder(state: MockState, id: DownloadId, folder: string): void {
  const row = state.row(id);
  row.path = `${folder}\\${row.file_name}`;
  if (row.error?.fix === 'choose_folder') {
    row.error = null;
    row.status = 'queued';
  }
  state.changed([row]);
}
