// The main window's link to the backend: loading it (the sample one only on demand, so it stays
// out of production), listening to its events and the shell's signals, and letting go of both.

import type { Reveal, UiEvent } from '@wirecatch/protocol';
import type { Backend, ShellSignal } from '../backend';

export class Connection {
  #loaded: Backend | null = null;
  #loading: Promise<Backend> | null = null;
  readonly #load: () => Promise<Backend>;
  #stop: (() => void) | null = null;

  /** Takes a backend, or a function that loads one. */
  constructor(backend: Backend | (() => Promise<Backend>)) {
    if (typeof backend === 'function') {
      this.#load = backend;
    } else {
      this.#loaded = backend;
      this.#load = () => Promise.resolve(backend);
    }
  }

  /** The backend, once `load` has finished; null before. */
  get loaded(): Backend | null {
    return this.#loaded;
  }

  /** The backend. Throws before `load` has finished. */
  get backend(): Backend {
    if (!this.#loaded) throw new Error('The backend is not loaded yet.');
    return this.#loaded;
  }

  /** Loads the backend, once: a second call while the first is still loading waits for the same backend rather than loading another. A load that failed is tried again by the next call (Try again). */
  load(): Promise<Backend> {
    this.#loading ??= this.#load().then(
      (backend) => {
        this.#loaded = backend;
        return backend;
      },
      (err: unknown) => {
        this.#loading = null;
        throw err;
      },
    );
    return this.#loading;
  }

  /**
   * Listens to the backend's events, the shell's signals and the downloads a clicked notification
   * shows. False, with nothing left listening, when `current` says this listener is no longer
   * wanted by the time it is set up; what arrives once it is no longer wanted is dropped.
   */
  async listen(
    current: () => boolean,
    onEvent: (e: UiEvent) => void,
    onSignal: (signal: ShellSignal) => void,
    onReveal: (reveal: Reveal) => void = () => undefined,
  ): Promise<boolean> {
    const backend = await this.load();
    const stops: (() => void)[] = [];
    const stop = () => {
      for (const one of stops) one();
    };
    try {
      // One at a time, so a listener that fails to start leaves the ones before it to stop.
      stops.push(
        await backend.subscribe(onEvent, () => {
          if (current()) onSignal('resync');
        }),
      );
      stops.push(
        await backend.onShellSignal((signal) => {
          if (current()) onSignal(signal);
        }),
      );
      stops.push(
        await backend.onReveal((reveal) => {
          if (current()) onReveal(reveal);
        }),
      );
    } catch (err) {
      // Nothing would ever close the listeners that did start otherwise.
      stop();
      throw err;
    }
    if (!current()) {
      stop();
      return false;
    }
    this.#stop = stop;
    return true;
  }

  /** The downloads a notification was about, clicked while this page was still loading: taken once the list is loaded, so they can be found in it. Only a selection is lost if it cannot be taken. */
  takeReveal(current: () => boolean, onReveal: (reveal: Reveal) => void): void {
    void this.backend.takeReveal().then(
      (reveal) => {
        if (reveal && current()) onReveal(reveal);
      },
      () => undefined,
    );
  }

  /** Stops listening. */
  close(): void {
    this.#stop?.();
    this.#stop = null;
  }
}
