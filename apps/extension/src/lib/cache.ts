import type { ExtensionSettings, QueueSummary } from '@wirecatch/protocol';
import type { CachedSettings, SettingsCache } from './hub/deps';
import {
  arrayOf,
  isBoolean,
  isNumber,
  isString,
  isUnknown,
  itemsOf,
  object,
  type Guard,
} from './validate';

/** Key of the display cache in `storage.local`. */
export const CACHE_KEY = 'wirecatch.cache.v1';

/** The part of `storage.local` the cache uses. */
export interface StorageArea {
  get(key: string): Promise<Record<string, unknown>>;
  set(items: Record<string, unknown>): Promise<void>;
}

const isStringList = arrayOf(isString);

const isSettings: Guard<ExtensionSettings> = object({
  capture_enabled: isBoolean,
  ask_before_download: isBoolean,
  show_media_button: isBoolean,
  ignored_sites: isStringList,
  capture_extensions: isStringList,
  min_capture_size: isNumber,
  default_queue: isNumber,
});

const isQueue: Guard<QueueSummary> = object({
  id: isNumber,
  name: isString,
  is_default: isBoolean,
});

/** `queues` and `appClosed` are read leniently: caches written before they existed lack them. */
const isEntry = object({
  settings: isSettings,
  savedAt: isNumber,
  queues: isUnknown,
  appClosed: isUnknown,
});

/** Reads a cache entry back, or null if it is missing or written by an incompatible version. */
export function asCachedSettings(value: unknown): CachedSettings | null {
  if (!isEntry(value)) return null;
  // Missing in caches written before it existed: then the app counts as running, which connects once to find out.
  return {
    settings: value.settings,
    queues: itemsOf(value.queues, isQueue),
    savedAt: value.savedAt,
    appClosed: value.appClosed === true,
  };
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
