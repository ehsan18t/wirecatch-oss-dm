// The sample's download complete window (D52), kept as the shell keeps it
// (`src-tauri/src/finished_window.rs`): while finished downloads are told in a window, what
// finishes or fails joins the newest group, and the window opens inside the page. A queue's
// summary is shown by a palette action, since the sample does not count queue runs.

import {
  MAX_FINISHED_GROUPS,
  MAX_FINISHED_LISTED,
  type DownloadId,
  type DownloadStatus,
  type DownloadSummary,
  type FinishedList,
  type FinishedQueue,
  type UiEvent,
} from '@wirecatch/protocol';
import { retryHelps } from '../../notify-options';
import type { FinishedSource, Unsubscribe } from '../types';
import type { MockState } from './state';

/** The label the sample page's complete window has, as the shell's does. */
export const FINISHED_LABEL = 'download-finished';
/** How many finished downloads the sample queue's run lists. */
const SAMPLE_QUEUE_RUN = 4;

export class MockFinished implements FinishedSource {
  #list: FinishedList = { groups: [] };
  #key = 0;
  readonly #listeners = new Set<(list: FinishedList) => void>();
  /** Each download's status as last seen, so only a change to finished or failed counts. */
  readonly #seen = new Map<DownloadId, DownloadStatus>();
  readonly #state: MockState;

  constructor(state: MockState) {
    this.#state = state;
    for (const row of state.rows.values()) this.#seen.set(row.id, row.status);
    state.listeners.add((event) => this.#observe(event));
    // Closing the window forgets what it listed, as the shell does.
    state.windows.subscribe((windows) => {
      if (windows.every((w) => w.label !== FINISHED_LABEL)) this.#list = { groups: [] };
    });
  }

  #observe(event: UiEvent): void {
    if (event.type !== 'downloads_changed') return;
    const ended: DownloadSummary[] = [];
    for (const row of event.data) {
      const before = this.#seen.get(row.id);
      this.#seen.set(row.id, row.status);
      if (before === row.status) continue;
      const tellsEach =
        this.#state.queues.find((q) => q.id === row.queue)?.settings.notify !== 'when_done';
      if (row.status === 'failed' || (row.status === 'completed' && tellsEach)) ended.push(row);
    }
    if (ended.length > 0 && this.#state.settings.notify_on_finish === 'window')
      this.add(null, ended);
  }

  /** Adds what ended, oldest first, and opens the window. */
  add(queue: FinishedQueue | null, ended: readonly DownloadSummary[]): void {
    const ids = new Set(ended.map((row) => row.id));
    const groups = this.#list.groups
      .map((g) => ({ ...g, downloads: g.downloads.filter((d) => !ids.has(d.id)) }))
      .filter((g) => g.downloads.length > 0);
    const newest = [...ended].reverse().map((row) => structuredClone(row));
    const top = groups[0];
    if (top && queue === null && top.queue === null) {
      top.downloads = [...newest, ...top.downloads].slice(0, MAX_FINISHED_LISTED);
    } else {
      this.#key += 1;
      groups.unshift({ key: this.#key, queue, downloads: newest.slice(0, MAX_FINISHED_LISTED) });
    }
    this.#list = { groups: groups.slice(0, MAX_FINISHED_GROUPS) };
    for (const listener of this.#listeners) listener(structuredClone(this.#list));
    this.#state.windows.showFinished(FINISHED_LABEL);
  }

  /** A palette action's: one finished download from the sample list. */
  sampleOne(): void {
    const row = [...this.#state.rows.values()].find((r) => r.status === 'completed');
    if (row) this.add(null, [row]);
  }

  /** A palette action's: a queue's run, a failure that trying again can fix among its downloads. */
  sampleQueueRun(): void {
    const rows = [...this.#state.rows.values()];
    const completed = rows.filter((r) => r.status === 'completed').slice(0, SAMPLE_QUEUE_RUN);
    const failed = rows.filter((r) => r.status === 'failed' && retryHelps(r)).slice(0, 1);
    const queue =
      this.#state.queues.find((q) => q.settings.notify === 'when_done') ?? this.#state.queues[0];
    if (!queue || completed.length + failed.length === 0) return;
    this.add(
      {
        id: queue.id,
        name: queue.settings.name,
        completed: completed.length,
        failed: failed.length,
      },
      [...completed, ...failed],
    );
  }

  finishedList(): Promise<FinishedList> {
    return Promise.resolve(structuredClone(this.#list));
  }

  onFinished(listener: (list: FinishedList) => void): Promise<Unsubscribe> {
    this.#listeners.add(listener);
    return Promise.resolve(() => void this.#listeners.delete(listener));
  }
}
