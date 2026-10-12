// The downloads waiting for the person's answer (D23). Each is asked in its own window; the
// main window only counts them ("Waiting for you") and never asks them itself.

import type { MediaChoice, PendingCapture } from '@wirecatch/protocol';
import type { Backend } from '../backend';

/** What the questions need from the window they are counted in. */
export interface QuestionsHost {
  backend: () => Backend;
}

export class Questions {
  /** Browser files waiting for an answer. */
  pending = $state<PendingCapture[]>([]);
  /** Videos waiting for a quality. */
  mediaChoices = $state<MediaChoice[]>([]);
  /** Video pages waiting for the media add-on, by question id. */
  addonPages = $state<number[]>([]);

  readonly #host: QuestionsHost;

  constructor(host: QuestionsHost) {
    this.#host = host;
  }

  /**
   * How many downloads wait for an answer: browser files, videos waiting for a quality, and
   * video pages waiting for the media add-on. Each counts once.
   */
  waitingCount = $derived.by(() => {
    const ids = [
      ...this.pending.map((p) => p.id),
      ...this.mediaChoices.map((c) => c.id),
      ...this.addonPages,
    ];
    return ids.filter((id, i) => ids.indexOf(id) === i).length;
  });

  /**
   * The questions open in the core: exactly what is open now, so one answered anywhere is never
   * counted again. Confirmations come with the main load.
   */
  async restore(): Promise<void> {
    const backend = this.#host.backend();
    try {
      this.mediaChoices = await backend.pendingMediaChoices();
    } catch {
      // The events still bring new choices.
    }
    try {
      const pages = await backend.pendingAddonRequests();
      this.addonPages = pages.map((p) => p.id);
    } catch {
      // Same: the events still bring new pages.
    }
  }

  confirmationRequested(capture: PendingCapture): void {
    if (!this.pending.some((p) => p.id === capture.id)) this.pending = [...this.pending, capture];
  }

  mediaChoiceRequested(choice: MediaChoice): void {
    if (!this.mediaChoices.some((c) => c.id === choice.id)) {
      this.mediaChoices = [...this.mediaChoices, choice];
    }
    this.#notWaitingForAddon(choice.id);
  }

  /** A page is being read once the add-on is in: no longer waiting for it. */
  analysing(id: number): void {
    this.#notWaitingForAddon(id);
  }

  addonRequired(id: number): void {
    // A quality list whose picked quality needs the add-on is already counted.
    if (!this.mediaChoices.some((c) => c.id === id) && !this.addonPages.includes(id)) {
      this.addonPages = [...this.addonPages, id];
    }
  }

  /** A question closed in the core (answered, dismissed or taken back): no longer counted. */
  closed(id: number): void {
    this.pending = this.pending.filter((p) => p.id !== id);
    this.mediaChoices = this.mediaChoices.filter((c) => c.id !== id);
    this.#notWaitingForAddon(id);
  }

  #notWaitingForAddon(id: number): void {
    this.addonPages = this.addonPages.filter((page) => page !== id);
  }
}
