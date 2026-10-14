interface Timer {
  remaining: number;
  startedAt: number;
  handle?: ReturnType<typeof setTimeout> | undefined;
}

/** Countdowns that can all be paused and resumed with the time each has left. */
export class PausableTimers {
  readonly #timers = new Map<string, Timer>();
  readonly #expire: (id: string) => void;
  #paused = false;

  constructor(expire: (id: string) => void) {
    this.#expire = expire;
  }

  start(id: string, duration: number): void {
    const timer: Timer = { remaining: duration, startedAt: 0 };
    this.#timers.set(id, timer);
    if (!this.#paused) this.#run(id, timer);
  }

  clear(id: string): void {
    clearTimeout(this.#timers.get(id)?.handle);
    this.#timers.delete(id);
  }

  setPaused(paused: boolean): void {
    if (paused === this.#paused) return;
    this.#paused = paused;
    for (const [id, timer] of this.#timers) {
      if (paused) this.#stop(timer);
      else this.#run(id, timer);
    }
  }

  dispose(): void {
    for (const timer of this.#timers.values()) clearTimeout(timer.handle);
    this.#timers.clear();
  }

  #run(id: string, timer: Timer): void {
    timer.startedAt = Date.now();
    timer.handle = setTimeout(() => {
      this.#timers.delete(id);
      this.#expire(id);
    }, timer.remaining);
  }

  #stop(timer: Timer): void {
    if (timer.handle === undefined) return;
    clearTimeout(timer.handle);
    timer.handle = undefined;
    timer.remaining -= Date.now() - timer.startedAt;
  }
}
