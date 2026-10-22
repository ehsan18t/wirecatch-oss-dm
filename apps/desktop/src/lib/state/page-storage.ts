// The webview's own storage, for the few display preferences that are this window's and not the app's settings: the look last shown, whether the sidebar is collapsed.

/** The page's storage, where the page may use it: none in tests without a window, and a private window may refuse it. */
export function pageStorage(): Storage | undefined {
  try {
    return 'localStorage' in globalThis ? globalThis.localStorage : undefined;
  } catch {
    return undefined;
  }
}

/** The text stored under `key`, or null when there is none or the storage cannot be read. */
export function readStored(key: string): string | null {
  try {
    return pageStorage()?.getItem(key) ?? null;
  } catch {
    return null;
  }
}

/** Stores `value` under `key`; storage can be unavailable, and then the preference is simply not kept. */
export function writeStored(key: string, value: string): void {
  try {
    pageStorage()?.setItem(key, value);
  } catch {
    // Not kept: the default shows next time.
  }
}
