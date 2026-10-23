import type { MediaKind } from '@wirecatch/protocol';
import { chooseTarget, describeTarget, popupTargets, type MediaTarget } from './choose';
import {
  MEDIA_BUTTON_QUERY,
  MEDIA_BUTTON_SETTING,
  MEDIA_CHECK,
  asContentRequest,
  type ButtonSetting,
  type CheckAnswer,
  type ElementRequest,
  type DownloadAnswer,
} from './messages';
import { fallback } from '../diag/log';
import { matchingSite } from '../sites';
import { parseUrl } from '../url';
import type { MediaRegistry, SavedMedia } from './registry';

/** A tab that cannot be read is treated as gone. */
const TAB_UNREADABLE = 'Could not read a tab';
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
  /**
   * Address of the frame the content script runs in, as the browser recorded it. In Chromium this is the address the frame had when the content script started: a page that changes its address with the history API (a single-page site such as YouTube) does not change it. Use `currentFrameUrl` for the address shown now.
   */
  url?: string;
  /** The frame the content script runs in (0 for the tab's page). */
  frameId?: number;
}

/** Media to hand to the app, with the page it belongs to. */
export interface MediaRequest {
  target: MediaTarget;
  title: string | null;
  pageUrl: string | null;
  /**
   * The full address of the frame the media is in. The hub trims it to what the
   * browser would send as `Referer` (`refererFor`) before it leaves the extension.
   */
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
  /** The "sites to ignore" setting: no media button there. */
  ignoredSites?(): readonly string[];
  /** Sites with video pages the person added to the ones the apps know (D51). */
  videoSites?(): readonly string[];
  /** "Show download button on videos", for a button asking as its page loads; on when missing. */
  mediaButtonOn?(): Promise<boolean>;
  /**
   * The tabs open now with the address each shows (null when it cannot be read), so saved lists of closed tabs, or of a page a tab has since left, are not restored.
   */
  liveTabs?(): Promise<{ id: number; url: string | null }[]>;
  /** How long changes are gathered before the lists are saved; `SAVE_DELAY_MS` when missing. */
  saveDelayMs?: number;
}

/** How long changes are gathered before the lists are saved, so a burst of detections is written once. */
const SAVE_DELAY_MS = 250;

export const PRIVATE_WINDOW = 'Wirecatch does not take media from private windows.';
export const IGNORED_SITE =
  'This site is on your list of sites to ignore. Remove it from that list in the Wirecatch popup to download from it.';
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

/** True when both are web addresses on the same origin (opaque origins never match). */
function sameWebOrigin(a: string, b: string): boolean {
  const first = parseUrl(a);
  const second = parseUrl(b);
  if (first === null || second === null) return false;
  return /^https?:$/.test(first.protocol) && first.origin === second.origin;
}

/**
 * The address the sender's frame shows now, which the media button acts on.
 *
 * The browser's `sender.url` is the frame's address when the content script started (Chromium keeps it from then on), so on a single-page site it still names the first video after the page moved on to another one with the history API. The history API cannot change a frame's origin, so a newer address on the same origin is taken from:
 *
 * 1. the tab's address (`sender.tab.url`), for the tab's own page (frame 0): the browser keeps it current;
 * 2. the address the content script reports (`location`), for any frame, such as an embedded player that changes its own address.
 *
 * An address on another origin is never taken: a page must not be able to make the button send an address on another site. Without a recorded `sender.url`, only the tab's own address is used for the tab's page, and nothing for a frame.
 */
export function currentFrameUrl(sender: ContentSender, location: string | null): string | null {
  const recorded = sender.url ?? null;
  const tabUrl = sender.frameId === 0 ? (sender.tab?.url ?? null) : null;
  if (recorded === null) return tabUrl;
  for (const candidate of [tabUrl, location]) {
    if (candidate !== null && sameWebOrigin(candidate, recorded)) return candidate;
  }
  return recorded;
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
      this.#deps.store
        ?.load()
        .catch(fallback('media', 'Could not read the saved media lists', null)),
      this.#deps.liveTabs?.().catch(fallback('media', 'Could not list the open tabs', undefined)),
    ]);
    // Called even without saved lists, so every open tab's page is known from the start.
    this.#deps.registry.restore(
      saved ?? null,
      live === undefined ? undefined : new Map(live.map((t) => [t.id, t.url])),
    );
  }

  /**
   * Answers a message from the media button. Returns null for messages that are not
   * for media (so other listeners can answer them).
   */
  handleContentMessage(
    raw: unknown,
    sender: ContentSender,
  ): Promise<CheckAnswer | DownloadAnswer | ButtonSetting> | null {
    const request = asContentRequest(raw);
    if (request === null) return null;
    if (request.type === MEDIA_BUTTON_QUERY) {
      const asking = this.#deps.mediaButtonOn?.() ?? Promise.resolve(true);
      return asking.then((on): ButtonSetting => ({ type: MEDIA_BUTTON_SETTING, on }));
    }
    return this.#ready.then((): Promise<CheckAnswer | DownloadAnswer> =>
      request.type === MEDIA_CHECK ? this.#check(request, sender) : this.#download(request, sender),
    );
  }

  /** What the button sends for an element in the frame that now shows `frameUrl` (`currentFrameUrl`). */
  #targetFor(
    request: ElementRequest,
    sender: ContentSender,
    frameUrl: string | null,
  ): MediaTarget | null {
    const tabId = sender.tab?.id;
    if (tabId === undefined || tabId < 0) return null;
    const tabUrl = sender.tab?.url ?? null;
    return chooseTarget(this.#deps.registry.itemsOn(tabId, tabUrl), {
      elementSrc: request.src,
      elementIsAudio: request.audio,
      frameUrl,
      tabUrl,
      frameId: sender.frameId ?? null,
      videoSites: this.#deps.videoSites?.() ?? [],
    });
  }

  /** True when the page or the frame is on a site the person told Wirecatch to ignore. */
  #ignored(sender: ContentSender): boolean {
    const sites = this.#deps.ignoredSites?.() ?? [];
    if (sites.length === 0) return false;
    return (
      matchingSite(sender.tab?.url ?? null, sites) !== null ||
      matchingSite(sender.url ?? null, sites) !== null
    );
  }

  #check(request: ElementRequest, sender: ContentSender): Promise<CheckAnswer> {
    if (sender.tab?.incognito === true || this.#ignored(sender))
      return Promise.resolve({ show: false });
    const frameUrl = currentFrameUrl(sender, request.location);
    return Promise.resolve({ show: this.#targetFor(request, sender, frameUrl) !== null });
  }

  async #download(request: ElementRequest, sender: ContentSender): Promise<DownloadAnswer> {
    const tab = sender.tab;
    if (tab?.incognito === true) return { ok: false, message: PRIVATE_WINDOW };
    if (this.#ignored(sender)) return { ok: false, message: IGNORED_SITE };
    const frameUrl = currentFrameUrl(sender, request.location);
    const target = this.#targetFor(request, sender, frameUrl);
    if (tab === undefined || target === null) return { ok: false, message: NOTHING_FOUND };
    const title = request.type === MEDIA_CHECK ? null : request.title;
    return this.#deps.send({
      target,
      title: cleanTitle(title) ?? cleanTitle(tab.title),
      pageUrl: tab.url ?? null,
      referrer: frameUrl ?? tab.url ?? null,
      cookieStoreId: tab.cookieStoreId ?? null,
    });
  }

  /** The popup's list for a tab. */
  async view(tabId: number): Promise<MediaView> {
    await this.#ready;
    const tab = await this.#deps.getTab(tabId).catch(fallback('media', TAB_UNREADABLE, null));
    if (!tab?.url || !/^https?:/i.test(tab.url)) {
      return { tabId, page: 'other', entries: [] };
    }
    if (tab.incognito) return { tabId, page: 'private', entries: [] };
    const entries = popupTargets(
      this.#deps.registry.itemsOn(tabId, tab.url),
      tab.url,
      this.#deps.videoSites?.() ?? [],
    ).map((target) => ({
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

  /**
   * The keyboard shortcut: sends the tab's best media, chosen like the button does
   * without an element (newest stream, then a file, then the page on a video site).
   */
  async downloadBest(tabId: number): Promise<MediaSendResult> {
    await this.#ready;
    const tab = await this.#deps.getTab(tabId).catch(fallback('media', TAB_UNREADABLE, null));
    if (tab === null) return { ok: false, message: TAB_GONE };
    if (tab.incognito) return { ok: false, message: PRIVATE_WINDOW };
    const sender: ContentSender = { tab: { id: tabId, url: tab.url ?? undefined }, frameId: 0 };
    if (this.#ignored(sender)) return { ok: false, message: IGNORED_SITE };
    const target = chooseTarget(this.#deps.registry.itemsOn(tabId, tab.url), {
      elementSrc: null,
      elementIsAudio: false,
      frameUrl: tab.url,
      tabUrl: tab.url,
      frameId: 0,
      videoSites: this.#deps.videoSites?.() ?? [],
    });
    if (target === null) return { ok: false, message: NOTHING_FOUND };
    return this.#deps.send({
      target,
      title: cleanTitle(tab.title),
      pageUrl: tab.url,
      referrer: tab.url,
      cookieStoreId: tab.cookieStoreId,
    });
  }

  /** Sends one item from the popup's list to the app. */
  async download(tabId: number, key: string): Promise<MediaSendResult> {
    await this.#ready;
    const tab = await this.#deps.getTab(tabId).catch(fallback('media', TAB_UNREADABLE, null));
    if (tab === null) return { ok: false, message: TAB_GONE };
    if (tab.incognito) return { ok: false, message: PRIVATE_WINDOW };
    const target = popupTargets(
      this.#deps.registry.itemsOn(tabId, tab.url),
      tab.url,
      this.#deps.videoSites?.() ?? [],
    ).find((t) => t.key === key);
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
      // Not saved, the lists last as long as this background script.
      store
        .save(this.#deps.registry.save())
        .catch(fallback('media', 'Could not save the media lists', undefined));
    }, this.#deps.saveDelayMs ?? SAVE_DELAY_MS);
  }
}
