import type { DetectedKind, Detection } from './detect';

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

/** What `save` produces and `restore` reads back (kept in `storage.session`). */
export interface SavedMedia {
  v: 1;
  tabs: Array<[number, MediaItem[]]>;
}

/**
 * How close together HLS playlists from one frame must come to count as one stream.
 * A player requests the master playlist and then the variant playlists it needs
 * within moments, and a live stream keeps requesting them, which keeps the window open.
 */
export const HLS_STREAM_WINDOW_MS = 10_000;

/** Most items kept per tab; the oldest is dropped to make room, since the newest is what plays now. */
export const MAX_ITEMS_PER_TAB = 30;

function parts(url: string): { origin: string; path: string } | null {
  try {
    const u = new URL(url);
    return { origin: u.origin, path: u.pathname };
  } catch {
    return null;
  }
}

export function itemKey(kind: DetectedKind, url: string): string {
  const p = parts(url);
  return `${kind}:${p === null ? url : p.origin + p.path}`;
}

/** A saved item; `frameId` and `lastActive` may be missing in lists saved by an earlier version. */
type SavedItem = Omit<MediaItem, 'frameId' | 'lastActive'> & { frameId?: number; lastActive?: number };

function isItem(value: unknown): value is SavedItem {
  if (typeof value !== 'object' || value === null) return false;
  const i = value as Record<string, unknown>;
  return (
    (i.kind === 'hls' || i.kind === 'dash' || i.kind === 'direct') &&
    typeof i.url === 'string' &&
    typeof i.key === 'string' &&
    typeof i.seenAt === 'number' &&
    typeof i.audio === 'boolean' &&
    (i.mime === null || typeof i.mime === 'string') &&
    (i.size === null || typeof i.size === 'number') &&
    (i.frameId === undefined || typeof i.frameId === 'number') &&
    (i.lastActive === undefined || typeof i.lastActive === 'number')
  );
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

  /** Records a detection from a frame of the tab; returns true when the tab's list changed. */
  add(tabId: number, detection: Detection, frameId = 0): boolean {
    const now = this.#now();
    const list = this.#tabs.get(tabId) ?? [];
    const key = itemKey(detection.kind, detection.url);
    const index = list.findIndex((i) => i.key === key);
    if (index >= 0) {
      const old = list[index];
      if (old === undefined) return false;
      old.lastActive = now;
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
    if (detection.kind === 'hls') {
      const stream = list.find(
        (i) => i.kind === 'hls' && i.frameId === frameId && now - i.lastActive <= HLS_STREAM_WINDOW_MS,
      );
      if (stream !== undefined) {
        // A variant of a listed stream: not listed, but it keeps the stream's window open.
        stream.lastActive = now;
        return false;
      }
    }
    const kept = list.length >= MAX_ITEMS_PER_TAB ? list.slice(list.length - MAX_ITEMS_PER_TAB + 1) : list;
    this.#set(tabId, [...kept, { ...detection, key, seenAt: now, frameId, lastActive: now }]);
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
      const savedItems = (entry[1] as unknown[])
        .filter(isItem)
        .map((i): MediaItem => ({ ...i, frameId: i.frameId ?? 0, lastActive: i.lastActive ?? i.seenAt }));
      const current = this.#tabs.get(tabId) ?? [];
      const merged = [...savedItems, ...current.filter((c) => !savedItems.some((s) => s.key === c.key))];
      if (merged.length === 0) continue;
      this.#set(tabId, merged.slice(-MAX_ITEMS_PER_TAB));
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
