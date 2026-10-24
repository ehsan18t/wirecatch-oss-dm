/** How many handover ids are remembered for spotting late answers. */
const RECENT_LIMIT = 50;

function remember<K, V>(map: Map<K, V>, key: K, value: V): void {
  map.set(key, value);
  if (map.size > RECENT_LIMIT) {
    const oldest = map.keys().next();
    if (oldest.done !== true) map.delete(oldest.value);
  }
}

/** A browser copy of a handover the app may have. */
interface KeptCopy {
  name: string;
  downloadId: number | null;
}

/** Tells the person a late answer matched a kept copy; `certain` is false for one that could not be read. */
export type LateMatch = (copy: KeptCopy, certain: boolean) => void;

/**
 * Matches handovers the browser kept although the app may have them (the withdraw
 * failed) with the app's confirmations that came after this side stopped waiting,
 * so the person is told once what became of the file. Whichever of the two comes
 * first is remembered (the most recent `RECENT_LIMIT` of each).
 */
export class DuplicateWatch {
  /** Handovers kept in the browser although the app may have them, by id. */
  readonly #uncertain = new Map<string, KeptCopy>();
  /** Handovers the app answered only after this side stopped waiting, and whether the answer was a sure confirmation. */
  readonly #lateConfirmed = new Map<string, { name: string; certain: boolean }>();
  readonly #matched: LateMatch;

  constructor(matched: LateMatch) {
    this.#matched = matched;
  }

  /** The browser kept a handover the app may have; returns true when the app had already answered it late (and tells). */
  keptUncertain(handoverId: string, copy: KeptCopy): boolean {
    const late = this.#lateConfirmed.get(handoverId);
    if (late !== undefined) {
      this.#lateConfirmed.delete(handoverId);
      this.#matched(copy, late.certain);
      return true;
    }
    remember(this.#uncertain, handoverId, copy);
    return false;
  }

  /**
   * The app answered a handover after this side gave up waiting; tells if the browser kept its
   * copy too. `certain` is false for an answer that could not be read, which may be a confirmation.
   */
  confirmedLate(handoverId: string, name: string, certain: boolean): void {
    const kept = this.#uncertain.get(handoverId);
    if (kept === undefined) {
      remember(this.#lateConfirmed, handoverId, { name, certain });
    } else {
      this.#uncertain.delete(handoverId);
      this.#matched(kept, certain);
    }
  }
}
