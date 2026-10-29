/**
 * Keeping diagnostics past a sleeping background script, and catching what nothing else did.
 *
 * The background script is stopped whenever it is idle, which would take its entries with it, so
 * the background and the extension's popup keep theirs in
 * `storage.session` under a key of their own (two writers on one key would overwrite each other). Session storage is cleared when the browser
 * closes and never leaves the machine. Content scripts keep theirs in memory only: they cannot
 * read session storage, and the page is theirs, not ours, to listen to.
 */
import { fields, isBoolean, isNumber, isString, itemsOf, member, nullable } from '../validate';
import {
  LEVELS,
  log,
  SCOPES,
  type DiagEntry,
  type Diagnostics,
  type Level,
  type Scope,
} from './log';

/** The storage key prefix; each context adds its name. */
export const DIAG_KEY_PREFIX = 'wirecatch.diag.v1.';

export type DiagContext = 'background' | 'popup' | 'notices';

/** The part of a `storage` area this needs. */
export interface DiagStore {
  get: (key: string) => Promise<Record<string, unknown>>;
  set: (items: Record<string, unknown>) => Promise<void>;
}

const isEntry = fields<DiagEntry>({
  at: isNumber,
  level: member<Level>(LEVELS),
  scope: member<Scope>(SCOPES),
  message: isString,
  detail: nullable(isString),
  repeated: isNumber,
  private: isBoolean,
});

const CONTEXTS: readonly DiagContext[] = ['background', 'popup', 'notices'];
const keyOf = (context: DiagContext): string => `${DIAG_KEY_PREFIX}${context}`;

/**
 * Restores the entries an earlier run of this context kept, and saves every new one. Writes are
 * one at a time, and one asked for while another runs writes the latest entries once it is done.
 */
export async function persistDiagnostics(
  diagnostics: Diagnostics,
  store: DiagStore,
  context: DiagContext,
): Promise<void> {
  const key = keyOf(context);
  let writing = false;
  let again = false;
  const write = (): void => {
    if (writing) {
      again = true;
      return;
    }
    writing = true;
    store
      .set({ [key]: diagnostics.entries() })
      .catch(() => {
        // Not logged: the entry it would make would be saved by a write that fails the same way.
        // The entries stay in memory, readable for as long as this context runs.
      })
      .finally(() => {
        writing = false;
        if (again) {
          again = false;
          write();
        }
      });
  };
  // Nothing is written before the earlier entries are back, or a write would replace them.
  const state = { restored: false, early: false };
  diagnostics.onChange(() => {
    if (state.restored) write();
    else state.early = true;
  });
  try {
    const saved = await store.get(key);
    diagnostics.restore(itemsOf(saved[key], isEntry));
  } catch (error) {
    log.warn('diag', 'Could not read the diagnostics kept before this start', error);
  }
  state.restored = true;
  if (state.early) write();
}

/**
 * Removes the private entries the other contexts saved, once the last private window has closed.
 * `writer`'s own key is left to its writer, which saves the entries it just filtered: writing it
 * here too, from an older read, could drop an entry logged meanwhile.
 */
export async function forgetPrivate(store: DiagStore, writer: DiagContext): Promise<void> {
  await Promise.all(
    CONTEXTS.filter((context) => context !== writer).map(async (context) => {
      const key = keyOf(context);
      const saved = itemsOf((await store.get(key))[key], isEntry);
      if (saved.some((e) => e.private)) await store.set({ [key]: saved.filter((e) => !e.private) });
    }),
  );
}

/** Every context's saved entries, oldest first, for a "copy diagnostics" action. */
export async function readDiagnostics(store: DiagStore): Promise<DiagEntry[]> {
  const saved = await Promise.all(
    CONTEXTS.map(async (context) =>
      itemsOf((await store.get(keyOf(context)))[keyOf(context)], isEntry),
    ),
  );
  return saved.flat().sort((a, b) => a.at - b.at);
}

/** The part of the global scope the error events come from: `self` in the background, `window` in a page. */
export interface ErrorEvents {
  addEventListener: ((
    type: 'error',
    listener: (event: { error?: unknown; message?: unknown }) => void,
  ) => void) &
    ((type: 'unhandledrejection', listener: (event: { reason?: unknown }) => void) => void);
}

/**
 * Logs every error nothing caught, and every rejected promise nothing handled (a `void`ed promise
 * included), as an error of `scope`. Only for the extension's own pages and background, never a
 * content script, whose global scope is the page's.
 */
export function logUncaught(target: ErrorEvents, scope: Scope): void {
  target.addEventListener('error', (event) => {
    log.error(scope, 'Uncaught error', event.error ?? event.message);
  });
  target.addEventListener('unhandledrejection', (event) => {
    log.error(scope, 'Unhandled rejection', event.reason);
  });
}
