// The add-ons as the main window shows them, and what the person can do with them.

import type { AddonInfo } from '@wirecatch/protocol';
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
        text: `A newer ${name} is ready (${media.latest_version ?? 'a new version'}). Choose Update in Settings, Add-ons to install it.`,
      };
    case 'installing':
      return { kind: 'info', text: `Found a newer ${name}. It is being installed now.` };
    case 'installed':
      // A newer release may be waiting until it is two days old; the core says so.
      return { kind: 'info', text: media.note ?? `The ${name} is up to date.` };
    case 'not_installed':
      return { kind: 'info', text: `The ${name} is not installed, so there is nothing to update.` };
    case 'failed':
      return { kind: 'error', text: media.state.data.message };
  }
}

export class Addons {
  /** Add-ons and their state; `null` until loaded. */
  list = $state<AddonInfo[] | null>(null);
  /** Why the add-ons could not be listed, shown in their settings section. */
  error = $state<string | null>(null);

  readonly #backend: () => Backend;
  readonly #runner: Runner;
  readonly #toasts: Toasts;

  constructor(backend: () => Backend, runner: Runner, toasts: Toasts) {
    this.#backend = backend;
    this.#runner = runner;
    this.#toasts = toasts;
  }

  /** Lists the add-ons. A failure is shown in their settings section, not as an error note. */
  async load(): Promise<void> {
    try {
      this.list = await this.#backend().listAddons();
      this.error = null;
    } catch (err) {
      this.error = errorMessage(err);
    }
  }

  /** The core's new list. */
  changed(addons: AddonInfo[]): void {
    this.list = addons;
    this.error = null;
  }

  install(id: string): Promise<boolean> {
    return this.#runner.succeeds(() => this.#backend().installAddon(id));
  }

  async update(id: string): Promise<void> {
    await this.#runner.run(() => this.#backend().updateAddon(id));
  }

  async remove(id: string): Promise<void> {
    await this.#runner.run(() => this.#backend().removeAddon(id));
  }

  /** Stops an install or update in progress; an update keeps the installed version. */
  async cancel(id: string): Promise<void> {
    await this.#runner.run(() => this.#backend().cancelAddonOperation(id));
  }

  /** Looks for a newer media add-on now and says what was found, in plain words. */
  async checkUpdates(): Promise<void> {
    const addons = await this.#runner.run(() => this.#backend().checkAddonUpdates());
    if (!addons) return;
    this.list = addons;
    const media = addons.find((a) => a.id === MEDIA_ADDON);
    if (!media) return;
    const note = updateNote(media);
    if (note.kind === 'error') this.#toasts.fail(note.text);
    else this.#toasts.notify(note.text);
  }
}
