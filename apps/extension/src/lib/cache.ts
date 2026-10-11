import type { ExtensionSettings, QueueSummary } from '@wirecatch/protocol';
import type { CachedSettings, SettingsCache } from './hub';

/** Key of the display cache in `storage.local`. */
export const CACHE_KEY = 'wirecatch.cache.v1';

/** The part of `storage.local` the cache uses. */
export interface StorageArea {
  get(key: string): Promise<Record<string, unknown>>;
  set(items: Record<string, unknown>): Promise<void>;
}

function isStringArray(value: unknown): value is string[] {
  return Array.isArray(value) && value.every((v) => typeof v === 'string');
}

function isSettings(value: unknown): value is ExtensionSettings {
  if (typeof value !== 'object' || value === null) return false;
  const s = value as Record<string, unknown>;
  return (
    typeof s.capture_enabled === 'boolean' &&
    typeof s.ask_before_download === 'boolean' &&
    typeof s.show_media_button === 'boolean' &&
    isStringArray(s.ignored_sites) &&
    isStringArray(s.capture_extensions) &&
    typeof s.min_capture_size === 'number' &&
    typeof s.default_queue === 'number'
  );
}

function isQueue(value: unknown): value is QueueSummary {
  if (typeof value !== 'object' || value === null) return false;
  const q = value as Record<string, unknown>;
  return (
    typeof q.id === 'number' && typeof q.name === 'string' && typeof q.is_default === 'boolean'
  );
}

/** Reads a cache entry back, or null if it is missing or written by an incompatible version. */
export function asCachedSettings(value: unknown): CachedSettings | null {
  if (typeof value !== 'object' || value === null) return null;
  const c = value as Record<string, unknown>;
  if (!isSettings(c.settings) || typeof c.savedAt !== 'number') return null;
  const queues = Array.isArray(c.queues) ? c.queues.filter(isQueue) : [];
  // Missing in caches written before it existed: then the app counts as running, which connects once to find out.
  return { settings: c.settings, queues, savedAt: c.savedAt, appClosed: c.appClosed === true };
}

export function storageCache(area: StorageArea): SettingsCache {
  return {
    async load() {
      const items = await area.get(CACHE_KEY);
      return asCachedSettings(items[CACHE_KEY]);
    },
    async save(value) {
      await area.set({ [CACHE_KEY]: value });
    },
  };
}
