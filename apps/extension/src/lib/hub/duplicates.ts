/** How many handover ids are remembered for spotting late answers. */
const RECENT_LIMIT = 50;

function remember<K, V>(map: Map<K, V>, key: K, value: V): void {
  map.set(key, value);
  if (map.size > RECENT_LIMIT) {
    const oldest = map.keys().next();
    if (oldest.done !== true) map.delete(oldest.value);
  }
}

/**
 * Matches handovers the browser kept although the app may have them (the withdraw
 * failed) with the app's confirmations that came after this side stopped waiting,
 * so the person is warned once when a file is downloading in both places. Whichever
 * of the two comes first is remembered (the most recent `RECENT_LIMIT` of each).
 */
export class DuplicateWatch {
  /** Handovers kept in the browser although the app may have them, by id, with the file's name. */
  readonly #uncertain = new Map<string, string>();
  /** Handovers the app confirmed only after this side stopped waiting. */
  readonly #lateConfirmed = new Map<string, string>();
  readonly #warn: (name: string) => void;

  constructor(warn: (name: string) => void) {
    this.#warn = warn;
  }

  /** The browser kept a handover the app may have; returns true when the app had already confirmed it (and warns). */
  keptUncertain(handoverId: string, name: string): boolean {
    if (this.#lateConfirmed.delete(handoverId)) {
      this.#warn(name);
      return true;
    }
    remember(this.#uncertain, handoverId, name);
    return false;
  }

  /** The app confirmed a handover after this side gave up waiting; warns if the browser kept its copy too. */
  confirmedLate(handoverId: string, name: string): void {
    if (this.#uncertain.delete(handoverId)) {
      this.#warn(name);
    } else {
      remember(this.#lateConfirmed, handoverId, name);
    }
  }
}
