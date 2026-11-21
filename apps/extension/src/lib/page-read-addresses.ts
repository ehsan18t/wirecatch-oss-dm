/**
 * The addresses the browser's connections reached for this extension's own page reads (D282), as `webRequest.onResponseStarted` reports them. `fetch()` resolves the host itself, so only this says where a read went; the app refuses an answer from the local network and does not use one without an address.
 */

/** How long an address is kept for the read that asks for it. */
const KEPT_MS = 60_000;
/** How long a read waits for its address after its answer arrived: `webRequest` reports it on its own schedule. */
export const ADDRESS_WAIT_MS = 1000;

/** One response of this extension's, as `webRequest.onResponseStarted` reports it in Chromium. */
export interface ReachedResponse {
  url: string;
  /** -1 for a request that belongs to no tab, as this extension's own do. */
  tabId: number;
  /** The origin that made the request: this extension's own for its page reads. */
  initiator?: string;
  /** The address the connection reached, when the browser knows it. */
  ip?: string;
}

interface Seen {
  url: string;
  ip: string;
  at: number;
}

export class ReachedAddresses {
  readonly #origin: string;
  readonly #now: () => number;
  #seen: Seen[] = [];
  readonly #waiting = new Set<() => void>();

  /** `origin` is this extension's own, `chrome-extension://<id>`. */
  constructor(origin: string, now: () => number) {
    this.#origin = origin;
    this.#now = now;
  }

  /** Notes where one of this extension's own requests went; every other response is left. */
  seen(response: ReachedResponse): void {
    if (response.tabId !== -1 || response.initiator !== this.#origin) return;
    if (response.ip === undefined || response.ip === '') return;
    const at = this.#now();
    this.#seen = this.#seen.filter((s) => at - s.at < KEPT_MS);
    this.#seen.push({ url: response.url, ip: response.ip, at });
    for (const wake of this.#waiting) wake();
  }

  /** The address the read of `url` started at `since` reached: null when none came within the wait, or when two reads of the same address went to different places. */
  async of(url: string, since: number): Promise<string | null> {
    const found = this.#found(url, since);
    if (found !== undefined) return found;
    return new Promise((resolve) => {
      const done = (address: string | null): void => {
        clearTimeout(timer);
        this.#waiting.delete(wake);
        resolve(address);
      };
      const wake = (): void => {
        const address = this.#found(url, since);
        if (address !== undefined) done(address);
      };
      const timer = setTimeout(() => done(null), ADDRESS_WAIT_MS);
      this.#waiting.add(wake);
    });
  }

  /** The one address seen for `url` since `since`, null for several, undefined for none yet. */
  #found(url: string, since: number): string | null | undefined {
    const ips = new Set(this.#seen.filter((s) => s.url === url && s.at >= since).map((s) => s.ip));
    if (ips.size === 0) return undefined;
    return ips.size === 1 ? [...ips][0] : null;
  }
}
