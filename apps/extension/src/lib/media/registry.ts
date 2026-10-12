import { MB } from '@wirecatch/units';
import type { DetectedKind, Detection } from './detect';
import {
  arrayOf,
  fields,
  isBoolean,
  isNumber,
  isString,
  isUnknown,
  literal,
  member,
  nullable,
  object,
  optional,
  refine,
  tuple,
} from '../validate';
import { MAX_MEDIA_URL_LENGTH } from './messages';

/** Media detected in a tab, in the order it was first seen. */
export interface MediaItem extends Detection {
  /** Stable within a tab: the kind plus the address without its query, so repeated requests count once. */
  key: string;
  /** When it was first seen (milliseconds since the epoch). */
  seenAt: number;
  /** The frame that loaded it (0 for the tab's page). */
  frameId: number;
  /** When it, or for HLS a playlist of the same stream, was last requested. */
  lastActive: number;
}

/**
 * What `save` produces and `restore` reads back (kept in `storage.session`): each tab's items, and the page they belong to when it is known.
 */
export interface SavedMedia {
  v: 1;
  tabs: [tabId: number, items: MediaItem[], page?: string][];
}

/**
 * How close together HLS playlists from one frame must come to count as one stream.
 * A player requests the master playlist and then the variant playlists it needs
 * within moments, and a live stream keeps requesting them, which keeps the window open.
 */
export const HLS_STREAM_WINDOW_MS = 10_000;

/** Most items kept per tab; the oldest is dropped to make room, since the newest is what plays now. */
export const MAX_ITEMS_PER_TAB = 30;

/**
 * Most characters of media lists written to `storage.session` at once (1 MB), well
 * inside its quota (10 MB in Chromium), which the in-flight download list shares.
 * The lists of the tabs that changed last are kept first.
 */
export const MAX_SAVED_CHARS = MB;

/** Room for the kind a key puts before its address (`direct:`), with some to spare. */
const KEY_PREFIX_ROOM = 16;

/** Characters a saved item adds besides its own JSON: its comma and its tab's share of the wrapping. */
const SAVED_ITEM_OVERHEAD = 16;

function parts(url: string): { origin: string; path: string } | null {
  try {
    const u = new URL(url);
    return { origin: u.origin, path: u.pathname };
  } catch {
    return null;
  }
}

/** A page's address without its fragment: a change of the fragment alone is not a new page. */
export function pageKey(url: string): string {
  const hash = url.indexOf('#');
  return hash < 0 ? url : url.slice(0, hash);
}

export function itemKey(kind: DetectedKind, url: string): string {
  const p = parts(url);
  return `${kind}:${p === null ? url : p.origin + p.path}`;
}

/** A saved item; `frameId` and `lastActive` may be missing in lists saved by an earlier version. */
type SavedItem = Omit<MediaItem, 'frameId' | 'lastActive'> & {
  frameId?: number;
  lastActive?: number;
};

const isSavedItem = fields<SavedItem>({
  kind: member<SavedItem['kind']>({ hls: true, dash: true, direct: true }),
  url: refine(isString, (url) => url.length <= MAX_MEDIA_URL_LENGTH),
  key: refine(isString, (key) => key.length <= MAX_MEDIA_URL_LENGTH + KEY_PREFIX_ROOM),
  seenAt: isNumber,
  audio: isBoolean,
  mime: nullable(isString),
  size: nullable(isNumber),
  frameId: optional(isNumber),
  lastActive: optional(isNumber),
});

/** A saved list as `save` writes it; the tabs are read one by one, so one damaged tab does not lose the others. */
const isSaved = object({ v: literal(1), tabs: arrayOf(isUnknown) });

/** One tab of a saved list; its items are read one by one, and a page that is not text counts as unknown. */
const isSavedTab = tuple(isNumber, arrayOf(isUnknown), isUnknown);

/**
 * True when an HLS playlist from this frame belongs to a stream already listed (see
 * `MediaRegistry`): it is not listed again, but it keeps the stream's window open.
 */
function extendsStream(list: readonly MediaItem[], frameId: number, now: number): boolean {
  const stream = list.find(
    (i) => i.kind === 'hls' && i.frameId === frameId && now - i.lastActive <= HLS_STREAM_WINDOW_MS,
  );
  if (stream === undefined) return false;
  stream.lastActive = now;
  return true;
}

function fromSaved(item: SavedItem): MediaItem {
  return { ...item, frameId: item.frameId ?? 0, lastActive: item.lastActive ?? item.seenAt };
}

/**
 * Media detected per tab. Dedupes repeated requests, lists only the first playlist of
 * an HLS stream (normally the master playlist), and forgets a tab's media when the
 * tab loads a new page or closes.
 *
 * HLS playlists cannot be told apart by address (variants live in any folder, on any
 * server), and their content is not read. A playlist counts as part of an already
 * listed stream when the same frame requested it within `HLS_STREAM_WINDOW_MS` of
 * that stream's last playlist request. Limits: two players in one frame starting
 * within moments of each other show as one stream, and a player that switches
 * quality after a long quiet stretch adds that variant as another entry.
 *
 * Each tab's list also records the page it was detected on, when that is known (`newPage`). `itemsOn` hands out a list only for that page, so a stale list (the tab moved on and the change has not been seen yet, or a list restored from before a change) is never offered on another page.
 */
export class MediaRegistry {
  readonly #tabs = new Map<number, MediaItem[]>();
  /** The page (`pageKey`) each tab's list belongs to, where it is known. */
  readonly #pages = new Map<number, string>();
  readonly #listeners = new Set<(tabId: number) => void>();
  readonly #now: () => number;

  constructor(options: { now?: () => number } = {}) {
    this.#now = options.now ?? Date.now;
  }

  /** Called with the tab id whenever a tab's list changes. */
  onChange(listener: (tabId: number) => void): () => void {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  }

  items(tabId: number): readonly MediaItem[] {
    return this.#tabs.get(tabId) ?? [];
  }

  /**
   * The tab's media, when the list belongs to the page at `pageUrl` (the address the tab shows now). Empty when the list was recorded for another page, or when the tab's address is unknown and the list's page is known.
   */
  itemsOn(tabId: number, pageUrl: string | null): readonly MediaItem[] {
    const page = this.#pages.get(tabId);
    if (page !== undefined && (pageUrl === null || pageKey(pageUrl) !== page)) return [];
    return this.items(tabId);
  }

  /** Records a detection from a frame of the tab; returns true when the tab's list changed. */
  add(tabId: number, detection: Detection, frameId = 0): boolean {
    if (detection.url.length > MAX_MEDIA_URL_LENGTH) return false;
    const now = this.#now();
    const list = this.#tabs.get(tabId) ?? [];
    const key = itemKey(detection.kind, detection.url);
    const index = list.findIndex((i) => i.key === key);
    if (index >= 0) return this.#seenAgain(tabId, index, detection, now);
    if (detection.kind === 'hls' && extendsStream(list, frameId, now)) return false;
    const kept =
      list.length >= MAX_ITEMS_PER_TAB ? list.slice(list.length - MAX_ITEMS_PER_TAB + 1) : list;
    this.#set(tabId, [...kept, { ...detection, key, seenAt: now, frameId, lastActive: now }]);
    return true;
  }

  /** The same file seen again: keeps the newest address (its query may carry a fresher token). */
  #seenAgain(tabId: number, index: number, detection: Detection, now: number): boolean {
    const list = this.items(tabId);
    const old = list[index];
    if (old === undefined) return false;
    old.lastActive = now;
    const merged: MediaItem = {
      ...old,
      url: detection.url,
      mime: detection.mime ?? old.mime,
      size: detection.size ?? old.size,
    };
    if (merged.url === old.url && merged.mime === old.mime && merged.size === old.size) {
      return false;
    }
    const next = [...list];
    next[index] = merged;
    this.#set(tabId, next);
    return true;
  }

  /** The tab shows a new page (`pageKey` of its address): forgets its media and records the page the next items belong to. */
  newPage(tabId: number, page: string): void {
    this.#pages.set(tabId, page);
    this.#forget(tabId);
  }

  /** Forgets a tab's media and its page (the tab closed). */
  clearTab(tabId: number): void {
    this.#pages.delete(tabId);
    this.#forget(tabId);
  }

  #forget(tabId: number): void {
    if (!this.#tabs.has(tabId)) return;
    this.#tabs.delete(tabId);
    this.#emit(tabId);
  }

  /**
   * The lists to keep in `storage.session`, at most `MAX_SAVED_CHARS` of JSON: the
   * tabs that changed last come first, and within a tab the newest items.
   */
  save(): SavedMedia {
    let budget = MAX_SAVED_CHARS;
    const kept: SavedMedia['tabs'] = [];
    for (const [id, items] of [...this.#tabs.entries()].reverse()) {
      const fit: MediaItem[] = [];
      for (let i = items.length - 1; i >= 0 && budget > 0; i -= 1) {
        const item = items[i];
        if (item === undefined) continue;
        const size = JSON.stringify(item).length + SAVED_ITEM_OVERHEAD;
        if (size > budget) {
          budget = 0;
          break;
        }
        budget -= size;
        fit.unshift(item);
      }
      const page = this.#pages.get(id);
      if (fit.length > 0) kept.unshift(page === undefined ? [id, fit] : [id, fit, page]);
      if (budget <= 0) break;
    }
    return { v: 1, tabs: kept };
  }

  /**
   * Puts back what a previous run of the background script saved. Tabs that already have media detected since the restart keep it, after the saved items.
   *
   * With `liveTabs` (each open tab's address now, null when it cannot be read), every open tab's page is recorded first, and a saved list is put back only when it was recorded for exactly that page. So the lists of closed tabs are dropped, and so is a list from a page the tab has since left (the save after that change was lost because the background stopped first), or whose page is unknown. Without `liveTabs`, a saved list is dropped only when the tab already has another page recorded.
   */
  restore(saved: unknown, liveTabs?: ReadonlyMap<number, string | null>): void {
    for (const [tabId, url] of liveTabs ?? []) {
      if (url !== null && !this.#pages.has(tabId)) this.#pages.set(tabId, pageKey(url));
    }
    if (!isSaved(saved)) return;
    for (const entry of saved.tabs) {
      if (!isSavedTab(entry)) continue;
      const [tabId, items, savedPage] = entry;
      const page = isString(savedPage) ? savedPage : undefined;
      if (!this.#mayRestore(tabId, page, liveTabs)) continue;
      this.#restoreTab(tabId, items.filter(isSavedItem).map(fromSaved), page);
    }
  }

  /** Whether a tab's saved list belongs to what the tab shows now (see `restore`). */
  #mayRestore(
    tabId: number,
    page: string | undefined,
    liveTabs: ReadonlyMap<number, string | null> | undefined,
  ): boolean {
    if (liveTabs !== undefined) return liveTabs.has(tabId) && page === this.#pages.get(tabId);
    const known = this.#pages.get(tabId);
    return known === undefined || page === known;
  }

  /** Puts a tab's saved items back, before what was detected since the restart. */
  #restoreTab(tabId: number, savedItems: MediaItem[], page: string | undefined): void {
    const current = this.#tabs.get(tabId) ?? [];
    const merged = [
      ...savedItems,
      ...current.filter((c) => !savedItems.some((s) => s.key === c.key)),
    ];
    if (merged.length === 0) return;
    if (page !== undefined) this.#pages.set(tabId, page);
    this.#set(tabId, merged.slice(-MAX_ITEMS_PER_TAB));
  }

  #set(tabId: number, items: MediaItem[]): void {
    // Re-inserted, so the map runs from the least to the most recently changed tab.
    this.#tabs.delete(tabId);
    this.#tabs.set(tabId, items);
    this.#emit(tabId);
  }

  #emit(tabId: number): void {
    for (const listener of this.#listeners) listener(tabId);
  }
}
