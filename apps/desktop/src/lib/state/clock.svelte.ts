// The time schedule labels and dates are worked out against. It moves on by itself while the
// window runs, from the system clock or from the sample clock of the mock backend.

import { MS_PER_SECOND } from '../time';

/**
 * How often the clock moves on. Labels show minutes, so twice a minute keeps them at most half a
 * minute late.
 */
const CLOCK_REFRESH_MS = 30 * MS_PER_SECOND;

/* eslint-disable svelte/prefer-svelte-reactivity -- `now` is replaced whole, never mutated. */

export class Clock {
  now = $state(new Date());
  #source: () => number = () => Date.now();
  #timer: ReturnType<typeof setInterval> | undefined;

  /** Reads the time from `source` from now on, starting now. */
  follow(source: () => number): void {
    this.#source = source;
    this.now = new Date(source());
  }

  /** Moves on by itself until `stop`. */
  start(): void {
    this.stop();
    this.#timer = setInterval(() => (this.now = new Date(this.#source())), CLOCK_REFRESH_MS);
  }

  stop(): void {
    if (this.#timer) clearInterval(this.#timer);
    this.#timer = undefined;
  }
}
