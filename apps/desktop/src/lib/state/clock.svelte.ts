// The time schedule labels and dates are worked out against. It moves on by itself while the
// window runs, from the system clock or from the sample clock of the mock backend.

import { dateAt, MS_PER_SECOND } from '../time';

/**
 * How often the clock moves on. Labels show minutes, so twice a minute keeps them at most half a
 * minute late.
 */
const CLOCK_REFRESH_MS = 30 * MS_PER_SECOND;

export class Clock {
  /** The time, in milliseconds since 1970: a plain number, replaced on every tick. */
  nowMs = $state(Date.now());
  #source: () => number = () => Date.now();
  #timer: ReturnType<typeof setInterval> | undefined;

  /** The time as a date, for the formatting helpers. A new one whenever the clock moves on. */
  get now(): Date {
    return dateAt(this.nowMs);
  }

  /** Reads the time from `source` from now on, starting now. */
  follow(source: () => number): void {
    this.#source = source;
    this.nowMs = source();
  }

  /** Moves on by itself until `stop`. */
  start(): void {
    this.stop();
    this.#timer = setInterval(() => (this.nowMs = this.#source()), CLOCK_REFRESH_MS);
  }

  stop(): void {
    if (this.#timer) clearInterval(this.#timer);
    this.#timer = undefined;
  }
}
