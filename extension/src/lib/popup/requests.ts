// The popup's requests on their way to the background script, by id. They are callbacks, not
// anything the popup shows, so they stay plain data outside its reactive state.

export class Requests<R> {
  readonly #waiting = new Map<number, (result: R) => void>();
  #nextId = 1;

  /** The id the next request will get. */
  get nextId(): number {
    return this.#nextId;
  }

  /** Starts a request: its id, and its result once `settle` gives one. */
  start(): { id: number; result: Promise<R> } {
    const id = this.#nextId++;
    const result = new Promise<R>((resolve) => this.#waiting.set(id, resolve));
    return { id, result };
  }

  /** Gives a request its result; an id that is not waiting (answered already, or unknown) is ignored. */
  settle(id: number, result: R): void {
    const done = this.#waiting.get(id);
    if (done === undefined) return;
    this.#waiting.delete(id);
    done(result);
  }

  /** Gives every waiting request the same result, for when the answers can no longer come. */
  settleAll(result: R): void {
    for (const id of this.#waiting.keys()) this.settle(id, result);
  }
}
