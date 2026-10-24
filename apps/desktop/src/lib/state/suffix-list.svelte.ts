// The Public Suffix List the app decides which addresses belong to one owner with (D44), as
// the settings show it, and the person's Check now.

import type { SuffixListInfo } from '@wirecatch/protocol';
import type { Backend } from '../backend';
import { errorMessage, type Runner } from './runner';
import type { Toasts } from './toasts.svelte';

/** What the list needs from the window it is shown in. */
export interface SuffixListHost {
  backend: () => Backend;
  runner: Runner;
  toasts: Toasts;
}

export class SuffixList {
  /** The list in use; `null` until loaded. */
  info = $state<SuffixListInfo | null>(null);
  /** Why it could not be read, shown in its setting. */
  error = $state<string | null>(null);
  /** A check is running, so Check now waits for it. */
  checking = $state(false);

  readonly #host: SuffixListHost;

  constructor(host: SuffixListHost) {
    this.#host = host;
  }

  /** Reads the list in use. A failure is shown in its setting, not as an error note. */
  async load(): Promise<void> {
    try {
      this.info = await this.#host.backend().suffixList();
      this.error = null;
    } catch (error) {
      this.error = errorMessage(error);
    }
  }

  /** The list after a check, the weekly one or a Check now, whether it worked or not. */
  checked(info: SuffixListInfo): void {
    this.info = info;
    this.error = null;
  }

  /** Looks for a newer list now and says what was found, in plain words. */
  async check(): Promise<void> {
    if (this.checking) return;
    this.checking = true;
    const before = this.info?.published;
    try {
      const info = await this.#host.runner.run(() => this.#host.backend().checkSuffixList());
      if (!info) return;
      this.checked(info);
      this.#host.toasts.notify(
        info.published === before
          ? 'The list of public domains is up to date.'
          : 'Wirecatch now uses a newer list of public domains.',
      );
    } finally {
      this.checking = false;
    }
  }
}
