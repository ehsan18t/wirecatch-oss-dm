// The main window's link to the backend: loading it (the sample one only on demand, so it stays
// out of production), listening to its events and the shell's signals, and letting go of both.

import type { UiEvent } from '@wirecatch/protocol';
import type { Backend, ShellSignal } from '../backend';

export class Connection {
  #loaded: Backend | null = null;
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

  /** The backend. Throws before `load` has finished. */
  get backend(): Backend {
    if (!this.#loaded) throw new Error('The backend is not loaded yet.');
    return this.#loaded;
  }

  /** Loads the backend, once. */
  async load(): Promise<Backend> {
    this.#loaded ??= await this.#load();
    return this.#loaded;
  }

  /**
   * Listens to the backend's events and the shell's signals. False, with nothing left listening,
   * when `current` says this listener is no longer wanted by the time it is set up; signals
   * arriving once it is no longer wanted are dropped.
   */
  async listen(
    current: () => boolean,
    onEvent: (e: UiEvent) => void,
    onSignal: (signal: ShellSignal) => void,
  ): Promise<boolean> {
    const backend = await this.load();
    const stopEvents = await backend.subscribe(onEvent);
    const stopSignals = await backend
      .onShellSignal((signal) => {
        if (current()) onSignal(signal);
      })
      .catch((err: unknown) => {
        // Nothing would ever close the event listener otherwise.
        stopEvents();
        throw err;
      });
    const stop = () => {
      stopEvents();
      stopSignals();
    };
    if (!current()) {
      stop();
      return false;
    }
    this.#stop = stop;
    return true;
  }

  /** Stops listening. */
  close(): void {
    this.#stop?.();
    this.#stop = null;
  }
}
