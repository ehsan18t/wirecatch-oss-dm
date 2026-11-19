/**
 * The browser's private session (D279): an id held while a private window is open, named in
 * `hello` and with every private capture, so the app forgets the session's downloads once it
 * ends. Kept in `storage.local`, so an update or a reload of the extension keeps the session; an
 * id found at start with no private window open belongs to a session that ended meanwhile.
 */
import { fallback } from './diag/log';
import type { StorageArea } from './cache';

/** Key of the session's id in `storage.session`. */
export const PRIVATE_SESSION_KEY = 'wirecatch.privateSession.v1';

export interface PrivateSessionDeps {
  area: StorageArea;
  /** Whether a private window is open now. */
  anyPrivateWindow: () => Promise<boolean>;
  newId: () => string;
  /** The session `id` ended: the browser's last private window closed. */
  ended: (id: string) => void;
}

export class PrivateSession {
  readonly #deps: PrivateSessionDeps;
  #id: string | null = null;
  readonly #loaded: Promise<void>;

  constructor(deps: PrivateSessionDeps) {
    this.#deps = deps;
    this.#loaded = deps.area
      .get(PRIVATE_SESSION_KEY)
      .then(async (items) => {
        const id = items[PRIVATE_SESSION_KEY];
        if (typeof id !== 'string' || id === '') return;
        this.#id = id;
        if (!(await deps.anyPrivateWindow())) this.#end();
      })
      .catch(fallback('settings', 'Could not read the private session', undefined));
  }

  /** The open session's id, started now if a private window is open without one; null when none is open. */
  async current(): Promise<string | null> {
    await this.#loaded;
    if (!(await this.#deps.anyPrivateWindow())) return null;
    if (this.#id === null) {
      this.#id = this.#deps.newId();
      this.#save();
    }
    return this.#id;
  }

  /** A window opened or closed: once no private window is left, the session ends. */
  async windowsChanged(): Promise<void> {
    await this.#loaded;
    if (this.#id === null || (await this.#deps.anyPrivateWindow())) return;
    this.#end();
  }

  #end(): void {
    const ended = this.#id;
    if (ended === null) return;
    this.#id = null;
    this.#save();
    this.#deps.ended(ended);
  }

  #save(): void {
    this.#deps.area
      .set({ [PRIVATE_SESSION_KEY]: this.#id })
      .catch(fallback('settings', 'Could not keep the private session', undefined));
  }
}
