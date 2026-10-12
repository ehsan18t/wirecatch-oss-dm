/** How long a message stays, long enough for screen readers to announce it. */
export const MESSAGE_MS = { ok: 4000, error: 8000 };

/**
 * The button's message ("Sent to Wirecatch."), in a live region so screen readers
 * announce it. It clears itself after `MESSAGE_MS`.
 */
export class LiveMessage {
  readonly #el: HTMLElement;
  /** Called when a message clears itself. */
  readonly #expired: () => void;
  #timer: ReturnType<typeof setTimeout> | null = null;

  constructor(el: HTMLElement, expired: () => void) {
    this.#el = el;
    this.#expired = expired;
  }

  get showing(): boolean {
    return this.#el.textContent !== '';
  }

  /** Shows a message, or clears it with an empty one. */
  say(message: string, error: boolean): void {
    this.#el.textContent = message;
    this.#el.classList.toggle('err', error);
    this.stop();
    if (message === '') return;
    this.#timer = setTimeout(
      () => {
        this.#timer = null;
        this.#el.textContent = '';
        this.#expired();
      },
      error ? MESSAGE_MS.error : MESSAGE_MS.ok,
    );
  }

  /** Cancels the pending clear. */
  stop(): void {
    if (this.#timer !== null) clearTimeout(this.#timer);
    this.#timer = null;
  }
}
