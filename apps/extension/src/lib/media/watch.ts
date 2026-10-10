import { classifyResponse, isNavigation, type ObservedResponse } from './detect';
import type { MediaRegistry } from './registry';

function withoutFragment(url: string): string {
  const hash = url.indexOf('#');
  return hash < 0 ? url : url.slice(0, hash);
}

export interface WatchDeps {
  /** True when the tab is in a private window. Rejects or resolves true when that cannot be told. */
  isPrivateTab(tabId: number): Promise<boolean>;
  /**
   * Settles once the lists saved by a previous run are restored (`MediaCenter.start`).
   * Responses wait for it, in order, so a navigation that woke the background clears
   * the restored list instead of being undone by it.
   */
  ready?: Promise<unknown>;
}

/**
 * Feeds `webRequest` responses into the registry: a new page in a tab clears its
 * list, media is recorded, and anything from a private window or outside a tab is
 * ignored, the same as downloads from private windows stay out of Wirecatch.
 *
 * Chromium and Firefox differences handled here and in the background script:
 *
 * - Background: Chromium runs a service worker and Firefox an event page. Both can
 *   be stopped when idle and are woken by the `webRequest` listener, which is
 *   registered synchronously at start for that reason. The lists are saved to
 *   `storage.session`, so a restarted background still knows each tab's media.
 * - Requests outside a tab (`tabId` -1): service workers, prefetches, the browser
 *   itself, other extensions. Both browsers report them; they are ignored, so media
 *   a site's service worker fetches is not seen.
 * - Private windows: Firefox marks each request (`incognito`); Chromium does not,
 *   so the tab is asked once (`tabs.get`) and the answer kept until the tab closes.
 * - Pages loading in the background (Chromium prerendering) are skipped; they are
 *   not shown to anyone yet.
 * - Navigation: a new document in the tab clears its list, and so does an address
 *   change without one (history API, back-forward cache), seen through
 *   `tabs.onUpdated`. Firefox lets the listener ask for address changes only.
 */
export class MediaWatcher {
  readonly #registry: MediaRegistry;
  readonly #deps: WatchDeps;
  /** Whether each tab is private; a tab never changes windows between private and normal. */
  readonly #private = new Map<number, boolean>();
  /** Bumped on every new page, so a detection that was waiting on the privacy check cannot land in the next page. */
  readonly #generation = new Map<number, number>();
  /** Each tab's address as last seen, without the fragment. */
  readonly #urls = new Map<number, string>();
  /** True once `deps.ready` settled; checked first so later responses do not wait a turn. */
  #restored = false;

  constructor(registry: MediaRegistry, deps: WatchDeps) {
    this.#registry = registry;
    this.#deps = deps;
  }

  async onResponse(response: ObservedResponse): Promise<void> {
    const { tabId } = response;
    if (tabId < 0) return;
    await this.#whenReady();
    if (isNavigation(response)) {
      this.#newPage(tabId);
      this.#urls.set(tabId, withoutFragment(response.url));
    }
    const detection = classifyResponse(response);
    if (detection === null) return;
    // Firefox says on each request; Chromium does not, so the tab is asked once.
    if (response.incognito === true) return;
    if (response.incognito === undefined) {
      const generation = this.#generation.get(tabId) ?? 0;
      if (await this.#isPrivate(tabId)) return;
      if ((this.#generation.get(tabId) ?? 0) !== generation) return;
    }
    this.#registry.add(tabId, detection);
  }

  /**
   * The tab's address changed (`tabs.onUpdated`). A full page load was already seen
   * as a navigation response, so this matters for pages that change their address
   * with the history API (single-page sites) and for pages restored from the
   * back-forward cache: both show other media without loading a new document.
   * A change of the fragment (`#...`) alone is not a new page.
   */
  async onUrlChange(tabId: number, url: string): Promise<void> {
    if (tabId < 0) return;
    await this.#whenReady();
    const address = withoutFragment(url);
    if (this.#urls.get(tabId) === address) return;
    this.#urls.set(tabId, address);
    this.#newPage(tabId);
  }

  #newPage(tabId: number): void {
    this.#generation.set(tabId, (this.#generation.get(tabId) ?? 0) + 1);
    this.#registry.clearTab(tabId);
  }

  async #whenReady(): Promise<void> {
    if (this.#restored || this.#deps.ready === undefined) return;
    await this.#deps.ready.catch(() => undefined);
    this.#restored = true;
  }

  onTabRemoved(tabId: number): void {
    this.#private.delete(tabId);
    this.#generation.delete(tabId);
    this.#urls.delete(tabId);
    this.#registry.clearTab(tabId);
  }

  async #isPrivate(tabId: number): Promise<boolean> {
    const known = this.#private.get(tabId);
    if (known !== undefined) return known;
    let isPrivate: boolean;
    try {
      isPrivate = await this.#deps.isPrivateTab(tabId);
    } catch {
      // Unknown tab (closed meanwhile, or a page loading in the background): treat as private.
      return true;
    }
    this.#private.set(tabId, isPrivate);
    return isPrivate;
  }
}
