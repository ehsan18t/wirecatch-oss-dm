import { fallback } from '../diag/log';
import {
  classifyResponse,
  isNavigation,
  requestedBy,
  sentCookies,
  type ObservedRequest,
  type ObservedResponse,
} from './detect';
import { pageKey, type MediaRegistry } from './registry';

/**
 * Most requests whose `Cookie` header is held while their answer is awaited. A request that
 * fails gets no answer, so the oldest are dropped; far more than a page has loading at once.
 */
export const MAX_UNANSWERED_REQUESTS = 256;

export interface WatchDeps {
  /** True when the tab is in a private window. Rejects or resolves true when that cannot be told. */
  isPrivateTab: (tabId: number) => Promise<boolean>;
  /** "Work in private windows" (D82). */
  privateWindowsOn: () => Promise<boolean>;
  /**
   * Settles once the lists saved by a previous run are restored (`MediaCenter.start`).
   * Responses wait for it, in order, so a navigation that woke the background clears
   * the restored list instead of being undone by it.
   */
  ready?: Promise<unknown>;
}

/**
 * Feeds `webRequest` responses into the registry: a new page in a tab clears its
 * list, media is recorded, and anything outside a tab is ignored, as is anything from a
 * private window while "Work in private windows" is off.
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
  /** The `Cookie` header of each request whose answer has not come yet, oldest first. */
  readonly #sent = new Map<string, string | null>();

  constructor(registry: MediaRegistry, deps: WatchDeps) {
    this.#registry = registry;
    this.#deps = deps;
  }

  /** A request is about to go out (`webRequest.onSendHeaders`): holds its `Cookie` header for its answer. */
  onRequest(request: ObservedRequest): void {
    if (request.tabId < 0) return;
    // Sent again after a redirect: the last hop's header is the one its answer came to.
    this.#sent.delete(request.requestId);
    this.#sent.set(request.requestId, sentCookies(request));
    for (const id of this.#sent.keys()) {
      if (this.#sent.size <= MAX_UNANSWERED_REQUESTS) break;
      this.#sent.delete(id);
    }
  }

  async onResponse(response: ObservedResponse): Promise<void> {
    const { tabId } = response;
    const cookies = this.#takeSent(response.requestId);
    if (tabId < 0) return;
    await this.#whenReady();
    if (isNavigation(response)) {
      const page = pageKey(response.url);
      this.#newPage(tabId, page);
      this.#urls.set(tabId, page);
    }
    const detection = classifyResponse(response);
    if (detection === null || (await this.#leftOut(response))) return;
    this.#registry.add(
      tabId,
      cookies === undefined ? detection : { ...detection, sentCookies: cookies },
      response.frameId ?? 0,
      requestedBy(response),
    );
  }

  /** The `Cookie` header held for a request (`onRequest`), released as its answer comes; undefined when it was not seen. */
  #takeSent(requestId: string): string | null | undefined {
    const cookies = this.#sent.get(requestId);
    this.#sent.delete(requestId);
    return cookies;
  }

  /** True when the response is from a private window left to the browser, or its tab moved to a new page while that was being asked. */
  async #leftOut(response: ObservedResponse): Promise<boolean> {
    const { tabId } = response;
    const generation = this.#generation.get(tabId) ?? 0;
    if ((await this.#fromPrivateWindow(response)) && !(await this.#deps.privateWindowsOn()))
      return true;
    return (this.#generation.get(tabId) ?? 0) !== generation;
  }

  async #fromPrivateWindow(response: ObservedResponse): Promise<boolean> {
    // Firefox says on each request; Chromium does not, so the tab is asked once.
    if (response.incognito !== undefined) return response.incognito;
    return this.#isPrivate(response.tabId);
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
    const address = pageKey(url);
    if (this.#urls.get(tabId) === address) return;
    this.#urls.set(tabId, address);
    this.#newPage(tabId, address);
  }

  /** Clears the tab's list and records the page its next media belongs to. */
  #newPage(tabId: number, page: string): void {
    this.#generation.set(tabId, (this.#generation.get(tabId) ?? 0) + 1);
    this.#registry.newPage(tabId, page);
  }

  async #whenReady(): Promise<void> {
    if (this.#restored || this.#deps.ready === undefined) return;
    await this.#deps.ready.catch(
      fallback('media', 'Restoring the media lists failed', undefined, 'error'),
    );
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
