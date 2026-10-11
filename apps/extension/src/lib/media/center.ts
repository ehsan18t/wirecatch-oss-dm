import type { MediaKind } from '@wirecatch/protocol';
import { chooseTarget, describeTarget, popupTargets, type MediaTarget } from './choose';
import {
  MEDIA_CHECK,
  asContentRequest,
  type CheckAnswer,
  type ContentRequest,
  type DownloadAnswer,
} from './messages';
import type { MediaRegistry, SavedMedia } from './registry';

/** What the background needs to know about a tab (`tabs.get`). */
export interface TabInfo {
  url: string | null;
  title: string | null;
  incognito: boolean;
  /** Firefox container or private cookie store of the tab; null in Chromium. */
  cookieStoreId: string | null;
}

/** Who sent a content script message (`runtime.MessageSender`). */
export interface ContentSender {
  tab?: {
    id?: number;
    url?: string;
    title?: string;
    incognito?: boolean;
    cookieStoreId?: string;
  };
  /** Address of the frame the content script runs in. */
  url?: string;
  /** The frame the content script runs in (0 for the tab's page). */
  frameId?: number;
}

/** Media to hand to the app, with the page it belongs to. */
export interface MediaRequest {
  target: MediaTarget;
  title: string | null;
  pageUrl: string | null;
  referrer: string | null;
  cookieStoreId: string | null;
}

/** The outcome of sending media, with a sentence for the person. */
export interface MediaSendResult {
  ok: boolean;
  message: string;
}

/** One row of the popup's "Media on this page" list. */
export interface MediaEntry {
  key: string;
  kind: MediaKind;
  title: string;
  detail: string;
  size: number | null;
}

/** The popup's list for one tab, and why it may be empty. */
export interface MediaView {
  tabId: number;
  /** `private`: a private window, where nothing is offered. `other`: not a web page. */
  page: 'web' | 'private' | 'other';
  entries: MediaEntry[];
}

/** Where the registry is saved, so a restarted background script still knows each tab's media. */
export interface MediaStore {
  load(): Promise<unknown>;
  save(value: SavedMedia): Promise<void>;
}

export interface MediaCenterDeps {
  registry: MediaRegistry;
  /** Null when the tab no longer exists. */
  getTab(tabId: number): Promise<TabInfo | null>;
  /** Hands media to the app (`Hub.sendMedia`). */
  send(request: MediaRequest): Promise<MediaSendResult>;
  store?: MediaStore;
  /** Ids of the tabs open now, so saved lists of closed tabs are not restored. */
  liveTabIds?(): Promise<number[]>;
  saveDelayMs?: number;
}

export const PRIVATE_WINDOW = 'Wirecatch does not take media from private windows.';
const NOTHING_FOUND =
  'Wirecatch has not found anything to download here yet. Play the video for a moment, then try again.';
const TAB_GONE = 'This tab is closed.';
const ITEM_GONE =
  'This item is no longer on the page. Close this window and open it again to see what is there now.';

const MAX_TITLE = 200;

function cleanTitle(title: string | null | undefined): string | null {
  const text = title?.replace(/\s+/g, ' ').trim() ?? '';
  return text === '' ? null : text.slice(0, MAX_TITLE);
}

/**
 * The background side of media: answers the media button, keeps open popups'
 * lists current, and sends what the person picked to the app.
 */
export class MediaCenter {
  readonly #deps: MediaCenterDeps;
  readonly #watchers = new Map<number, Set<(view: MediaView) => void>>();
  #saveTimer: ReturnType<typeof setTimeout> | null = null;
  /** Settles once the saved lists are back; questions about a tab wait for it. */
  #ready: Promise<void> = Promise.resolve();

  constructor(deps: MediaCenterDeps) {
    this.#deps = deps;
    deps.registry.onChange((tabId) => {
      this.#scheduleSave();
      const watchers = this.#watchers.get(tabId);
      if (watchers === undefined || watchers.size === 0) return;
      void this.view(tabId).then((view) => {
        for (const push of watchers) push(view);
      });
    });
  }

  /**
   * Puts back the media a previous run of the background script saw. The returned
   * promise is what the `MediaWatcher` waits for before handling any response, so the
   * request that woke the background (often a new page) is applied on top of the
   * restored lists instead of being overwritten by them.
   */
  start(): Promise<void> {
    this.#ready = this.#restore();
    return this.#ready;
  }

  async #restore(): Promise<void> {
    const [saved, live] = await Promise.all([
      this.#deps.store?.load().catch(() => null),
      this.#deps.liveTabIds?.().catch(() => undefined),
    ]);
    if (saved != null) this.#deps.registry.restore(saved, live === undefined ? undefined : new Set(live));
  }

  /**
   * Answers a message from the media button. Returns null for messages that are not
   * for media (so other listeners can answer them).
   */
  handleContentMessage(raw: unknown, sender: ContentSender): Promise<CheckAnswer | DownloadAnswer> | null {
    const request = asContentRequest(raw);
    if (request === null) return null;
    return this.#ready.then((): Promise<CheckAnswer | DownloadAnswer> =>
      request.type === MEDIA_CHECK ? this.#check(request, sender) : this.#download(request, sender),
    );
  }

  #targetFor(request: ContentRequest, sender: ContentSender): MediaTarget | null {
    const tabId = sender.tab?.id;
    if (tabId === undefined || tabId < 0) return null;
    return chooseTarget(this.#deps.registry.items(tabId), {
      elementSrc: request.src,
      elementIsAudio: request.audio,
      frameUrl: sender.url ?? null,
      tabUrl: sender.tab?.url ?? null,
      frameId: sender.frameId ?? null,
    });
  }

  #check(request: ContentRequest, sender: ContentSender): Promise<CheckAnswer> {
    if (sender.tab?.incognito === true) return Promise.resolve({ show: false });
    return Promise.resolve({ show: this.#targetFor(request, sender) !== null });
  }

  async #download(request: ContentRequest, sender: ContentSender): Promise<DownloadAnswer> {
    const tab = sender.tab;
    if (tab?.incognito === true) return { ok: false, message: PRIVATE_WINDOW };
    const target = this.#targetFor(request, sender);
    if (tab === undefined || target === null) return { ok: false, message: NOTHING_FOUND };
    const title = request.type === MEDIA_CHECK ? null : request.title;
    return this.#deps.send({
      target,
      title: cleanTitle(title) ?? cleanTitle(tab.title),
      pageUrl: tab.url ?? null,
      referrer: sender.url ?? tab.url ?? null,
      cookieStoreId: tab.cookieStoreId ?? null,
    });
  }

  /** The popup's list for a tab. */
  async view(tabId: number): Promise<MediaView> {
    await this.#ready;
    const tab = await this.#deps.getTab(tabId).catch(() => null);
    if (tab === null || tab.url === null || !/^https?:/i.test(tab.url)) {
      return { tabId, page: 'other', entries: [] };
    }
    if (tab.incognito) return { tabId, page: 'private', entries: [] };
    const entries = popupTargets(this.#deps.registry.items(tabId), tab.url).map((target) => ({
      key: target.key,
      kind: target.kind,
      size: target.size,
      ...describeTarget(target),
    }));
    return { tabId, page: 'web', entries };
  }

  /** Sends the tab's list now and after every change, until the returned function is called. */
  watch(tabId: number, push: (view: MediaView) => void): () => void {
    let set = this.#watchers.get(tabId);
    if (set === undefined) {
      set = new Set();
      this.#watchers.set(tabId, set);
    }
    set.add(push);
    void this.view(tabId).then(push);
    return () => {
      const current = this.#watchers.get(tabId);
      current?.delete(push);
      if (current?.size === 0) this.#watchers.delete(tabId);
    };
  }

  /** Sends one item from the popup's list to the app. */
  async download(tabId: number, key: string): Promise<MediaSendResult> {
    await this.#ready;
    const tab = await this.#deps.getTab(tabId).catch(() => null);
    if (tab === null) return { ok: false, message: TAB_GONE };
    if (tab.incognito) return { ok: false, message: PRIVATE_WINDOW };
    const target = popupTargets(this.#deps.registry.items(tabId), tab.url).find((t) => t.key === key);
    if (target === undefined) return { ok: false, message: ITEM_GONE };
    return this.#deps.send({
      target,
      title: cleanTitle(tab.title),
      pageUrl: tab.url,
      referrer: tab.url,
      cookieStoreId: tab.cookieStoreId,
    });
  }

  #scheduleSave(): void {
    const store = this.#deps.store;
    if (store === undefined || this.#saveTimer !== null) return;
    this.#saveTimer = setTimeout(() => {
      this.#saveTimer = null;
      store.save(this.#deps.registry.save()).catch(() => {
        // Session storage unavailable: the lists then last as long as this background script.
      });
    }, this.#deps.saveDelayMs ?? 250);
  }
}
