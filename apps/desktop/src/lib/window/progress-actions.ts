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

  /**
   * Cancel on a download: stops it, deletes the partial file and takes it off the list. A
   * finished download has nothing to cancel. The core deletes an unfinished download's partial
   * files on any removal, and a finished one's file only when asked, deciding by the status it
   * holds at that moment; so the file is never asked for, and one that finished before the
   * window heard of it is kept.
   */
  async cancelDownload(): Promise<void> {
    if (this.#host.row()?.status === 'completed') return;
    if (await this.#onRow((id) => this.#host.backend().remove([id], false))) this.#host.close();
  }

  /** Opens the finished file; for a program the shell first asks whether the person trusts where it came from. */
  openFile(): Promise<boolean> {
    return this.#onRow((id) => this.#host.backend().openFile(id));
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
      case 'proxy_settings':
        await this.#host.runner.run(() => this.#host.backend().showProxySettings());
        break;
    }
  }
}
