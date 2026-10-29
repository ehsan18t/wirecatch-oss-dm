// The add-ons as the main window shows them, and what the person can do with them.

import type { AddonId, AddonInfo } from '@wirecatch/protocol';
import { MEDIA_ADDON } from '../addons';
import type { Backend } from '../backend';
import { errorMessage, type Runner } from './runner';
import type { Toasts } from './toasts.svelte';

/** What checking for updates found, in plain words, by the state the add-on is in now. */
function updateNote(media: AddonInfo): { kind: 'info' | 'error'; text: string } {
  const name = media.name.toLowerCase();
  switch (media.state.type) {
    case 'update_available':
      return {
        kind: 'info',
        text: `A newer ${name} is ready (${media.latest_version ?? 'a new version'}). Choose Update in Settings > Media to install it.`,
      };
    case 'installing':
      return { kind: 'info', text: `A newer ${name} was found and is being installed.` };
    case 'installed':
      // A newer release may be waiting until it is two days old; the core says so.
      return { kind: 'info', text: media.note ?? `The ${name} is up to date.` };
    case 'not_installed':
      return { kind: 'info', text: `The ${name} is not installed, so there is nothing to update.` };
    case 'failed':
      return { kind: 'error', text: media.state.data.message };
  }
}

/** What the add-ons need from the window they are shown in. */
export interface AddonsHost {
  backend: () => Backend;
  runner: Runner;
  toasts: Toasts;
}

export class Addons {
  /** Add-ons and their state; `null` until loaded. */
  list = $state<AddonInfo[] | null>(null);
  /** Why the add-ons could not be listed, shown in their settings section. */
  error = $state<string | null>(null);

  readonly #host: AddonsHost;

  constructor(host: AddonsHost) {
    this.#host = host;
  }

  /** Lists the add-ons. A failure is shown in their settings section, not as an error note. */
  async load(): Promise<void> {
    try {
      this.list = await this.#host.backend().listAddons();
      this.error = null;
    } catch (error) {
      this.error = errorMessage(error);
    }
  }

  /** The core's new list. */
  changed(addons: AddonInfo[]): void {
    this.list = addons;
    this.error = null;
  }

  install(id: AddonId): Promise<boolean> {
    return this.#host.runner.succeeds(() => this.#host.backend().installAddon(id));
  }

  async update(id: AddonId): Promise<void> {
    await this.#host.runner.run(() => this.#host.backend().updateAddon(id));
  }

  async remove(id: AddonId): Promise<void> {
    await this.#host.runner.run(() => this.#host.backend().removeAddon(id));
  }

  /** Stops an install or update in progress; an update keeps the installed version. */
  async cancel(id: AddonId): Promise<void> {
    await this.#host.runner.run(() => this.#host.backend().cancelAddonOperation(id));
  }

  /** Looks for a newer media add-on now and says what was found, in plain words. */
  async checkUpdates(): Promise<void> {
    const addons = await this.#host.runner.run(() => this.#host.backend().checkAddonUpdates());
    if (!addons) return;
    this.list = addons;
    const media = addons.find((a) => a.id === MEDIA_ADDON);
    if (!media) return;
    const note = updateNote(media);
    if (note.kind === 'error') this.#host.toasts.fail(note.text);
    else this.#host.toasts.notify(note.text);
  }
}
