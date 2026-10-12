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
  /** Handovers kept in the browser although the app may have them, by id, with the file's name and browser download. */
  readonly #uncertain = new Map<string, { name: string; downloadId: number | null }>();
  /** Handovers the app answered only after this side stopped waiting, and whether the answer was a sure confirmation. */
  readonly #lateConfirmed = new Map<string, { name: string; certain: boolean }>();
  readonly #warn: (name: string, certain: boolean, downloadId: number | null) => void;

  constructor(warn: (name: string, certain: boolean, downloadId: number | null) => void) {
    this.#warn = warn;
  }

  /** The browser kept a handover the app may have; returns true when the app had already confirmed it (and warns). */
  keptUncertain(handoverId: string, name: string, downloadId: number | null): boolean {
    const late = this.#lateConfirmed.get(handoverId);
    if (late !== undefined) {
      this.#lateConfirmed.delete(handoverId);
      this.#warn(name, late.certain, downloadId);
      return true;
    }
    remember(this.#uncertain, handoverId, { name, downloadId });
    return false;
  }

  /**
   * The app answered a handover after this side gave up waiting; warns if the browser kept its
   * copy too. `certain` is false for an answer that could not be read, which may be a confirmation.
   */
  confirmedLate(handoverId: string, name: string, certain: boolean): void {
    const kept = this.#uncertain.get(handoverId);
    if (kept !== undefined) {
      this.#uncertain.delete(handoverId);
      this.#warn(kept.name, certain, kept.downloadId);
    } else {
      remember(this.#lateConfirmed, handoverId, { name, certain });
    }
  }
}
