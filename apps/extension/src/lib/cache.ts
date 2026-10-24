import { isExtensionSettings } from '@wirecatch/protocol';
import type { CachedSettings, SettingsCache } from './hub/deps';
import { isUnknown, object } from './validate';

/** Key of the copy of the app's settings kept for the browser session, in `storage.session` (D61). */
export const CACHE_KEY = 'wirecatch.session.v1';

/** What earlier versions kept in `storage.local`: removed when the extension is installed or updated, so nothing of the app's settings outlives the browser session (D61). */
export const RETIRED_LOCAL_KEYS: readonly string[] = [
  'wirecatch.cache.v1',
  'wirecatch.mediaButton.v1',
];

/** The part of a storage area the copy uses. */
export interface StorageArea {
  get: (key: string) => Promise<Record<string, unknown>>;
  set: (items: Record<string, unknown>) => Promise<void>;
}

/** `appClosed` is read leniently: one that does not read counts as false, so waking connects once to find out. */
const isEntry = object({ settings: isExtensionSettings, appClosed: isUnknown });

/** Reads a copy back, or null if it is missing or written in another shape. */
export function asCachedSettings(value: unknown): CachedSettings | null {
  if (!isEntry(value)) return null;
  return { settings: value.settings, appClosed: value.appClosed === true };
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

/** A storage area that lives as long as the background script, for a browser without `storage.session`. */
export function memoryArea(): StorageArea {
  const items = new Map<string, unknown>();
  return {
    get(key) {
      return Promise.resolve(items.has(key) ? { [key]: items.get(key) } : {});
    },
    set(values) {
      for (const [key, value] of Object.entries(values)) items.set(key, value);
      return Promise.resolve();
    },
  };
}
