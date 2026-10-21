// The actions on downloads: adding, pausing, resuming, trying again, cancelling and removing them,
// opening their files, the fixes their errors offer, and their place in the queues. Each says
// what went wrong in a note; a bulk action says it once.

import type { AddRequest, DownloadId, FixAction, QueueId } from '@wirecatch/protocol';
import type { Backend } from '../backend';
import { linkForDisplay } from '../format';
import type { DownloadStore } from './downloads.svelte';
import { nudged, placed } from './order';
import { canPause, canResume, canRetry } from './row-actions';
import type { Queues } from './queues.svelte';
import { errorMessage, type Runner } from './runner';
import type { Dialog } from './screens';
import type { Selection } from './selection.svelte';
import type { Toasts } from './toasts.svelte';
import { groupOf, isInLine, type Sort } from './view';

/** What the actions need from the window they run in. */
export interface DownloadActionsHost {
  backend: () => Backend;
  runner: Runner;
  toasts: Toasts;
  downloads: DownloadStore;
  selection: Selection;
  queues: Queues;
  openDialog: (dialog: Dialog) => void;
  /** What the list shows and how it is sorted: nudging works in one queue, in its order. */
  /** The list's sort: a queue's order changes only while the list shows it. */
  sort: () => Sort;
}

/** What adding gave: the new downloads, and each link that did not become one, with why. */
export interface Added {
  ids: DownloadId[];
  failed: { url: string; error: string }[];
}

/** What Space did: which action it took, and on how many downloads it succeeded. `verb` is null when nothing in the selection could take either. */
export interface Toggled {
  verb: 'pause' | 'resume' | null;
  done: number;
}

export class DownloadActions {
  readonly #host: DownloadActionsHost;

  constructor(host: DownloadActionsHost) {
    this.#host = host;
  }

  /** Adds the downloads one after another, in the order given (a queue keeps it), and notes how many were added. What failed is given back with why, for the form that asked to show. */
  async add(requests: AddRequest[]): Promise<Added> {
    const ids: DownloadId[] = [];
    const failed: Added['failed'] = [];
    for (const r of requests) {
      try {
        ids.push(await this.#host.backend().addDownload(r));
      } catch (err) {
        failed.push({ url: r.url, error: errorMessage(err) });
      }
    }
    if (ids.length > 0) {
      this.#host.toasts.notify(
        ids.length === 1 ? 'Download added.' : `${ids.length} downloads added.`,
      );
    }
    return { ids, failed };
  }

  async pause(ids: DownloadId[] = this.#host.selection.list): Promise<void> {
    await this.#host.runner.bulk(ids, 'pause', (id) => this.#host.backend().pause(id));
  }

  async resume(ids: DownloadId[] = this.#host.selection.list): Promise<void> {
    await this.#host.runner.bulk(ids, 'resume', (id) => this.#host.backend().resume(id));
  }

  /** Space bar: pause what runs, or else resume what stopped, each download only if it takes the action. Says what it did, so nothing claims a change that did not happen. */
  async togglePause(ids: DownloadId[] = this.#host.selection.list): Promise<Toggled> {
    const rows = ids.flatMap((id) => this.#host.downloads.get(id) ?? []);
    const backend = this.#host.backend();
    const running = rows.filter(canPause).map((r) => r.id);
    if (running.length > 0) {
      const done = await this.#host.runner.bulk(running, 'pause', (id) => backend.pause(id));
      return { verb: 'pause', done };
    }
    const stopped = rows.filter((r) => canResume(r) || canRetry(r));
    if (stopped.length === 0) return { verb: null, done: 0 };
    // A cancelled download has no partial file left; it starts over.
    const cancelled = stopped.filter((r) => r.status === 'cancelled').map((r) => r.id);
    // One bulk action, so a failure is said once and every success is counted.
    const done = await this.#host.runner.bulk(
      stopped.map((r) => r.id),
      'resume',
      (id) => (cancelled.includes(id) ? backend.retry(id) : backend.resume(id)),
    );
    return { verb: 'resume', done };
  }

  async retry(ids: DownloadId[] = this.#host.selection.list): Promise<void> {
    await this.#host.runner.bulk(ids, 'restart', (id) => this.#host.backend().retry(id));
  }

  /** Tries every download that needs attention again. */
  async retryAllFailed(): Promise<void> {
    const failed = this.#host.downloads.order.filter(
      (id) => this.#host.downloads.get(id)?.status === 'failed',
    );
    if (failed.length === 0) {
      this.#host.toasts.notify('Nothing needs attention right now.');
      return;
    }
    const ok = await this.#host.runner.bulk(failed, 'restart', (id) =>
      this.#host.backend().retry(id),
    );
    if (ok > 0)
      this.#host.toasts.notify(
        ok === 1 ? 'Trying 1 download again.' : `Trying ${ok} downloads again.`,
      );
  }

  async cancel(ids: DownloadId[] = this.#host.selection.list): Promise<void> {
    await this.#host.runner.bulk(ids, 'cancel', (id) => this.#host.backend().cancel(id));
  }

  async remove(ids: DownloadId[], deleteFile: boolean): Promise<void> {
    await this.#host.runner.run(() => this.#host.backend().remove(ids, deleteFile));
  }

  refreshLink(id: DownloadId, url: string): Promise<boolean> {
    return this.#host.runner.succeeds(() => this.#host.backend().refreshLink(id, url));
  }

  /** Opens a finished file; for a program the shell first asks whether the person trusts where it came from. */
  async openFile(id: DownloadId): Promise<void> {
    await this.#host.runner.run(() => this.#host.backend().openFile(id));
  }

  async openFolder(id: DownloadId): Promise<void> {
    await this.#host.runner.run(() => this.#host.backend().openFolder(id));
  }

  /** Copies a download's link, with any password in it hidden, and says whether that worked. */
  async copyLink(id: DownloadId): Promise<void> {
    const row = this.#host.downloads.get(id);
    if (!row) return;
    try {
      await navigator.clipboard.writeText(linkForDisplay(row.url));
      this.#host.toasts.notify('Link copied.');
    } catch {
      this.#host.toasts.fail(
        'Could not copy the link. Select it in the detail panel and copy it by hand.',
      );
    }
  }

  /** Lets the person pick a new folder for one download. */
  async chooseFolderFor(id: DownloadId): Promise<boolean> {
    const row = this.#host.downloads.get(id);
    const picked = await this.#host.runner.run(() => this.#host.backend().pickFolder(row?.path));
    if (!picked) return false;
    const ok = await this.#host.runner.succeeds(() =>
      this.#host.backend().setDownloadFolder(id, picked),
    );
    if (ok) this.#host.toasts.notify(`Saving to ${picked}`);
    return ok;
  }

  /** Runs the fix an error offers. */
  async fix(id: DownloadId, action: FixAction): Promise<void> {
    switch (action) {
      case 'retry':
        await this.retry([id]);
        break;
      case 'refresh_link':
        this.#host.openDialog({ kind: 'refresh', id });
        break;
      case 'choose_folder':
        if (await this.chooseFolderFor(id)) await this.retry([id]);
        break;
      case 'free_disk_space':
        await this.openFolder(id);
        break;
      case 'keep_original':
        await this.#host.runner.run(() => this.#host.backend().keepOriginal(id));
        break;
    }
  }

  async moveToQueue(ids: DownloadId[], queue: QueueId): Promise<void> {
    if (ids.length === 0) return;
    const ok = await this.#host.runner.succeeds(() => this.#host.backend().moveToQueue(ids, queue));
    const name = this.#host.queues.byId(queue)?.name ?? 'the queue';
    if (ok)
      this.#host.toasts.notify(
        ids.length === 1 ? `Moved to ${name}.` : `${ids.length} downloads moved to ${name}.`,
      );
  }

  /**
   * Moves `ids` inside their queue so they sit just before `before` (or at the end when it is
   * `null`). Only downloads of that one queue take part.
   */
  async placeInQueue(queue: QueueId, ids: DownloadId[], before: DownloadId | null): Promise<void> {
    const next = placed(this.#host.downloads.idsInQueue(queue), ids, before);
    if (next === null) return;
    if (await this.#host.runner.succeeds(() => this.#host.backend().reorder(queue, next))) {
      this.#host.downloads.reorder(queue, next);
    }
  }

  /** Alt+Up and Alt+Down: nudge the selection one place up or down among the downloads of its queue in its own group, as the list shows them. */
  async nudge(delta: -1 | 1): Promise<void> {
    if (this.#host.sort().column !== 'position') {
      this.#host.toasts.notify('Sort the list by # to change the order of a queue.');
      return;
    }
    const rows = this.#host.selection.rows.filter((r) => isInLine(r.status));
    const first = rows[0];
    if (first === undefined) return;
    const group = groupOf(first.status);
    if (rows.some((r) => r.queue !== first.queue || groupOf(r.status) !== group)) {
      this.#host.toasts.notify(
        'Select downloads of one queue and one group to change their order.',
      );
      return;
    }
    const line = this.#host.downloads
      .idsInQueue(first.queue)
      .filter((id) => isInLine(this.#host.downloads.get(id)?.status ?? 'completed'));
    const inGroup = line.filter((id) => {
      const status = this.#host.downloads.get(id)?.status;
      return status !== undefined && groupOf(status) === group;
    });
    const move = nudged(inGroup, (id) => this.#host.selection.has(id), delta);
    if (!move) return;
    // Past the group's last download: just after it in the whole line, ahead of the next group's.
    const lastOfGroup = inGroup.at(-1);
    const before =
      move.before ??
      (lastOfGroup === undefined
        ? null
        : (line.slice(line.indexOf(lastOfGroup) + 1).find((id) => !move.chosen.includes(id)) ??
          null));
    await this.placeInQueue(first.queue, move.chosen, before);
  }

  async pauseAll(): Promise<void> {
    await this.#host.runner.run(() => this.#host.backend().pauseAll());
  }

  async resumeAll(): Promise<void> {
    await this.#host.runner.run(() => this.#host.backend().resumeAll());
  }
}
