import type { DetectedKind, Detection } from './detect';

/** Media detected in a tab, in the order it was first seen. */
export interface MediaItem extends Detection {
  /** Stable within a tab: the kind plus the address without its query, so repeated requests count once. */
  key: string;
  /** When it was first seen (milliseconds since the epoch). */
  seenAt: number;
}

/** What `save` produces and `restore` reads back (kept in `storage.session`). */
export interface SavedMedia {
  v: 1;
  tabs: Array<[number, MediaItem[]]>;
}

/** Most items kept per tab; later ones are ignored, since the first ones are the page's main media. */
export const MAX_ITEMS_PER_TAB = 30;

function parts(url: string): { origin: string; path: string } | null {
  try {
    const u = new URL(url);
    return { origin: u.origin, path: u.pathname };
  } catch {
    return null;
  }
}

function directory(path: string): string {
  return path.slice(0, path.lastIndexOf('/') + 1);
}

/**
 * True when two HLS playlists look like parts of one stream: same server, and one
 * sits in the other's folder or below it (for example `video/master.m3u8` and
 * `video/720p/index.m3u8`). The app reads the master playlist and offers every
 * quality, so only the first playlist of a stream is listed.
 */
function sameStream(a: string, b: string): boolean {
  const pa = parts(a);
  const pb = parts(b);
  if (pa === null || pb === null || pa.origin !== pb.origin) return false;
  const da = directory(pa.path);
  const db = directory(pb.path);
  return da.startsWith(db) || db.startsWith(da);
}

export function itemKey(kind: DetectedKind, url: string): string {
  const p = parts(url);
  return `${kind}:${p === null ? url : p.origin + p.path}`;
}

function isItem(value: unknown): value is MediaItem {
  if (typeof value !== 'object' || value === null) return false;
  const i = value as Record<string, unknown>;
  return (
    (i.kind === 'hls' || i.kind === 'dash' || i.kind === 'direct') &&
    typeof i.url === 'string' &&
    typeof i.key === 'string' &&
    typeof i.seenAt === 'number' &&
    typeof i.audio === 'boolean' &&
    (i.mime === null || typeof i.mime === 'string') &&
    (i.size === null || typeof i.size === 'number')
  );
}

/**
 * Media detected per tab. Dedupes repeated requests, lists only the first playlist of
 * an HLS stream (normally the master playlist), and forgets a tab's media when the
 * tab loads a new page or closes.
 */
export class MediaRegistry {
  readonly #tabs = new Map<number, MediaItem[]>();
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

  /** Records a detection; returns true when the tab's list changed. */
  add(tabId: number, detection: Detection): boolean {
    const list = this.#tabs.get(tabId) ?? [];
    const key = itemKey(detection.kind, detection.url);
    const index = list.findIndex((i) => i.key === key);
    if (index >= 0) {
      const old = list[index];
      if (old === undefined) return false;
      // The same file again: keep the newest address (its query may carry a fresher token).
      const merged: MediaItem = {
        ...old,
        url: detection.url,
        mime: detection.mime ?? old.mime,
        size: detection.size ?? old.size,
      };
      if (merged.url === old.url && merged.mime === old.mime && merged.size === old.size) return false;
      const next = [...list];
      next[index] = merged;
      this.#set(tabId, next);
      return true;
    }
    if (detection.kind === 'hls' && list.some((i) => i.kind === 'hls' && sameStream(i.url, detection.url))) {
      return false;
    }
    if (list.length >= MAX_ITEMS_PER_TAB) return false;
    this.#set(tabId, [...list, { ...detection, key, seenAt: this.#now() }]);
    return true;
  }

  /** Forgets a tab's media (it loaded a new page or closed). */
  clearTab(tabId: number): void {
    if (!this.#tabs.has(tabId)) return;
    this.#tabs.delete(tabId);
    this.#emit(tabId);
  }

  save(): SavedMedia {
    return { v: 1, tabs: [...this.#tabs.entries()].map(([id, items]) => [id, [...items]]) };
  }

  /**
   * Puts back what a previous run of the background script saved. Tabs that already
   * have media detected since the restart keep it, after the saved items. With
   * `liveTabs`, tabs that no longer exist are dropped.
   */
  restore(saved: unknown, liveTabs?: ReadonlySet<number>): void {
    if (typeof saved !== 'object' || saved === null) return;
    const { v, tabs } = saved as { v?: unknown; tabs?: unknown };
    if (v !== 1 || !Array.isArray(tabs)) return;
    for (const entry of tabs as unknown[]) {
      if (!Array.isArray(entry) || typeof entry[0] !== 'number' || !Array.isArray(entry[1])) continue;
      const tabId = entry[0];
      if (liveTabs !== undefined && !liveTabs.has(tabId)) continue;
      const savedItems = (entry[1] as unknown[]).filter(isItem);
      const current = this.#tabs.get(tabId) ?? [];
      const merged = [...savedItems, ...current.filter((c) => !savedItems.some((s) => s.key === c.key))];
      if (merged.length === 0) continue;
      this.#set(tabId, merged.slice(0, MAX_ITEMS_PER_TAB));
    }
  }

  #set(tabId: number, items: MediaItem[]): void {
    this.#tabs.set(tabId, items);
    this.#emit(tabId);
  }

  #emit(tabId: number): void {
    for (const listener of this.#listeners) listener(tabId);
  }
}
