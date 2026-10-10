import { classifyResponse, isNavigation, type ObservedResponse } from './detect';
import type { MediaRegistry } from './registry';

export interface WatchDeps {
  /** True when the tab is in a private window. Rejects or resolves true when that cannot be told. */
  isPrivateTab(tabId: number): Promise<boolean>;
}

/**
 * Feeds `webRequest` responses into the registry: a new page in a tab clears its
 * list, media is recorded, and anything from a private window or outside a tab is
 * ignored, the same as downloads from private windows stay out of Wirecatch.
 */
export class MediaWatcher {
  readonly #registry: MediaRegistry;
  readonly #deps: WatchDeps;
  /** Whether each tab is private; a tab never changes windows between private and normal. */
  readonly #private = new Map<number, boolean>();
  /** Bumped on every new page, so a detection that was waiting on the privacy check cannot land in the next page. */
  readonly #generation = new Map<number, number>();

  constructor(registry: MediaRegistry, deps: WatchDeps) {
    this.#registry = registry;
    this.#deps = deps;
  }

  async onResponse(response: ObservedResponse): Promise<void> {
    const { tabId } = response;
    if (tabId < 0) return;
    if (isNavigation(response)) {
      this.#generation.set(tabId, (this.#generation.get(tabId) ?? 0) + 1);
      this.#registry.clearTab(tabId);
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

  onTabRemoved(tabId: number): void {
    this.#private.delete(tabId);
    this.#generation.delete(tabId);
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
