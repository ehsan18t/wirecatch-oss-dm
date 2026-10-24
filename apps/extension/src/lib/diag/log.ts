/**
 * The extension's logger. Every error the extension catches and handles goes through here, so a
 * failure in the field can be told apart from one that never happened (docs/ENGINEERING.md,
 * section 6). Entries are kept in a small ring buffer that a "copy diagnostics" action can read,
 * and in development they are also written to the console (`console.ts`, the one place allowed to).
 *
 * Levels mean what they mean in the Rust code: `error`, a person sees a failure; `warn`, something
 * went wrong and was handled; `info`, a lifecycle event; `debug`, for development only, never kept
 * in a production build.
 */
import { writeToConsole } from './console';
import { describeError, redact } from './redact';

/** Every level, as a record so a guard can list them all (`member` in validate.ts). */
export const LEVELS = { error: true, warn: true, info: true, debug: true } as const;
export type Level = keyof typeof LEVELS;

/** The parts of the extension an entry can come from. */
export const SCOPES = {
  background: true,
  capture: true,
  diag: true,
  hub: true,
  media: true,
  native: true,
  notes: true,
  notices: true,
  page: true,
  popup: true,
  settings: true,
} as const;
export type Scope = keyof typeof SCOPES;

export interface DiagEntry {
  /** Milliseconds since the epoch. */
  at: number;
  level: Level;
  scope: Scope;
  /** What failed, in a sentence written in the code; redacted anyway. */
  message: string;
  /** The redacted error, when there was one. */
  detail: string | null;
  /** How many times the same entry came again since the first; `at` is the last time. */
  repeated: number;
  /** Made while a private window was open, so it is forgotten when the last one closes. */
  private: boolean;
}

/** Entries kept per context: enough for the story of a failure, small enough for session storage. */
export const DIAG_CAPACITY = 100;
/** How far back an entry is looked for to count a repeat, so one failing hot path fills one entry, not the buffer. */
const REPEAT_WINDOW = 10;

/** Whether two entries say the same thing, whenever they were made. */
const same = (a: DiagEntry, b: DiagEntry): boolean =>
  a.level === b.level &&
  a.scope === b.scope &&
  a.message === b.message &&
  a.detail === b.detail &&
  a.private === b.private;

export interface DiagnosticsOptions {
  capacity?: number;
  now?: () => number;
  /** Keep `debug` entries too; only in development. */
  keepDebug?: boolean;
  /** Also hands every entry to this, as it is made; the development build writes it to the console. */
  echo?: ((entry: DiagEntry) => void) | null;
}

/** A bounded, ordered record of what went wrong, oldest first. */
export class Diagnostics {
  readonly #capacity: number;
  readonly #now: () => number;
  readonly #keepDebug: boolean;
  readonly #echo: ((entry: DiagEntry) => void) | null;
  #entries: DiagEntry[] = [];
  /** Whether a private window is open; null while this context does not know yet. */
  #privateSession: boolean | null = false;
  /** The entries made while that was not known, which take the first answer. */
  #madeUnknowing = new WeakSet<DiagEntry>();
  readonly #listeners = new Set<() => void>();

  constructor(options: DiagnosticsOptions = {}) {
    this.#capacity = options.capacity ?? DIAG_CAPACITY;
    this.#now = options.now ?? Date.now;
    this.#keepDebug = options.keepDebug ?? false;
    this.#echo = options.echo ?? null;
  }

  log(level: Level, scope: Scope, message: string, error?: unknown): void {
    const entry: DiagEntry = {
      at: this.#now(),
      level,
      scope,
      message: redact(message),
      detail: error === undefined ? null : describeError(error),
      repeated: 0,
      private: this.#privateSession !== false,
    };
    this.#echo?.(entry);
    if (level === 'debug' && !this.#keepDebug) return;
    const recent = this.#entries.slice(-REPEAT_WINDOW);
    // While it is not known whether a private window is open, a repeat counts only into an entry
    // made meanwhile: counted into a restored private one, it would carry that one's count out.
    const unknowing = this.#privateSession === null;
    const earlier = recent.findLast(
      (e) => same(e, entry) && (!unknowing || this.#madeUnknowing.has(e)),
    );
    const kept = earlier === undefined ? this.#entries : this.#entries.filter((e) => e !== earlier);
    const added =
      earlier === undefined ? entry : { ...earlier, at: entry.at, repeated: earlier.repeated + 1 };
    if (unknowing) this.#madeUnknowing.add(added);
    this.#entries = [...kept, added].slice(-this.#capacity);
    this.#changed();
  }

  /**
   * Whether a private window is open, or null while that is not known. While one is, or may be,
   * every entry is marked private, since what it is about cannot be told. Whenever none is, the
   * marked entries are forgotten, restored ones included, except those made while it was not
   * known: they were made with no private window open, and are kept as not private.
   */
  setPrivateSession(open: boolean | null): void {
    this.#privateSession = open;
    if (open === null) return;
    const unknowing = this.#madeUnknowing;
    this.#madeUnknowing = new WeakSet();
    if (open) return;
    const entries = this.#entries;
    this.#entries = entries.flatMap((e) => {
      if (!e.private) return [e];
      return unknowing.has(e) ? [{ ...e, private: false }] : [];
    });
    if (this.#entries.some((e, i) => e !== entries[i]) || this.#entries.length !== entries.length) {
      this.#changed();
    }
  }

  /** Every entry kept, oldest first. */
  entries(): DiagEntry[] {
    return [...this.#entries];
  }

  /**
   * Puts entries kept by an earlier run of this context before the ones made since. Private ones
   * are dropped when no private window is open: the session they belonged to has ended.
   */
  restore(earlier: readonly DiagEntry[]): void {
    const kept = this.#privateSession === false ? earlier.filter((e) => !e.private) : earlier;
    this.#entries = [...kept, ...this.#entries].slice(-this.#capacity);
    if (kept.length < earlier.length) this.#changed();
  }

  /** Forgets every entry, as a "clear diagnostics" action would. */
  clear(): void {
    this.#entries = [];
    this.#changed();
  }

  /** Calls `listener` whenever the kept entries change: one is added, repeated or forgotten. */
  onChange(listener: () => void): () => void {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  }

  #changed(): void {
    for (const listener of this.#listeners) listener();
  }
}

/** Development builds, but not the tests, which check the console writer on its own. */
const DEVELOPMENT = import.meta.env.DEV && import.meta.env.MODE !== 'test';

/** This context's diagnostics: the background's, the popup's or a content script's. */
export const diagnostics = new Diagnostics({
  keepDebug: DEVELOPMENT,
  echo: DEVELOPMENT ? writeToConsole : null,
});

export const log = {
  error: (scope: Scope, message: string, error?: unknown): void => {
    diagnostics.log('error', scope, message, error);
  },
  warn: (scope: Scope, message: string, error?: unknown): void => {
    diagnostics.log('warn', scope, message, error);
  },
  info: (scope: Scope, message: string, error?: unknown): void => {
    diagnostics.log('info', scope, message, error);
  },
  debug: (scope: Scope, message: string, error?: unknown): void => {
    diagnostics.log('debug', scope, message, error);
  },
};

/**
 * A rejection handler that logs the error and answers `value` instead: the fallback stays what it
 * was, and the failure is no longer silent. `promise.catch(fallback('hub', 'Could not read the
 * cache', null))`.
 */
export function fallback<T>(
  scope: Scope,
  message: string,
  value: T,
  level: Level = 'warn',
): (error: unknown) => T {
  return (error) => {
    diagnostics.log(level, scope, message, error);
    return value;
  };
}
