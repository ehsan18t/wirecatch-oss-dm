// What a download window does with the download it shows: pause, resume, try again, cancel,
// open it, and the fixes its error offers. Each says what went wrong in the window.

import type { DownloadId, DownloadSummary, FixAction } from '@wirecatch/protocol';
import type { Backend } from '../backend';
import type { Runner } from '../state/runner';

/** What the actions need from the window they run in. */
export interface ProgressActionsHost {
  backend: () => Backend;
  runner: Runner;
  /** The download shown, once there is one. */
  downloadId: () => DownloadId | null;
  row: () => DownloadSummary | null;
  /** Asks for a folder, starting at `current`. */
  pickFolder: (current: string) => Promise<string | null>;
  close: () => void;
}

export class ProgressActions {
  readonly #host: ProgressActionsHost;

  constructor(host: ProgressActionsHost) {
    this.#host = host;
  }

  /** Runs an action on the download shown; false when there is none or the action failed. */
  async #onRow(action: (id: DownloadId) => Promise<void>): Promise<boolean> {
    const id = this.#host.downloadId();
    return id !== null && (await this.#host.runner.succeeds(() => action(id)));
  }

  pause(): Promise<boolean> {
    return this.#onRow((id) => this.#host.backend().pause(id));
  }

  resume(): Promise<boolean> {
    // A cancelled download has no partial file left; it starts over.
    return this.#onRow((id) =>
      this.#host.row()?.status === 'cancelled'
        ? this.#host.backend().retry(id)
        : this.#host.backend().resume(id),
    );
  }

  retry(): Promise<boolean> {
    return this.#onRow((id) => this.#host.backend().retry(id));
  }

  /** Cancel on a download: stops it, deletes the partial file and takes it off the list. */
  async cancelDownload(): Promise<void> {
    if (await this.#onRow((id) => this.#host.backend().remove([id], true))) this.#host.close();
  }

  /** Whether opening the finished file could run code, so the window asks first. */
  async isProgram(): Promise<boolean | undefined> {
    const id = this.#host.downloadId();
    if (id === null) return undefined;
    return this.#host.runner.run(() => this.#host.backend().isProgram(id));
  }

  openFile(allowProgram = false): Promise<boolean> {
    return this.#onRow((id) => this.#host.backend().openFile(id, allowProgram));
  }

  openFolder(): Promise<boolean> {
    return this.#onRow((id) => this.#host.backend().openFolder(id));
  }

  refreshLink(url: string): Promise<boolean> {
    return this.#onRow((id) => this.#host.backend().refreshLink(id, url));
  }

  /** Runs the fix an error offers. A fresh link needs the link: `askLink` shows that form. */
  async fix(action: FixAction, askLink: () => void): Promise<void> {
    switch (action) {
      case 'retry':
        await this.retry();
        break;
      case 'refresh_link':
        askLink();
        break;
      case 'choose_folder': {
        const row = this.#host.row();
        if (!row) return;
        const picked = await this.#host.pickFolder(row.path);
        if (!picked) return;
        if (await this.#onRow((id) => this.#host.backend().setDownloadFolder(id, picked))) {
          await this.retry();
        }
        break;
      }
      case 'free_disk_space':
        await this.openFolder();
        break;
      case 'keep_original':
        await this.#onRow((id) => this.#host.backend().keepOriginal(id));
        break;
    }
  }
}
