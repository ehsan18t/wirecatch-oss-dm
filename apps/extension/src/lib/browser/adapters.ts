/**
 * The browser APIs the background script hands to the hub and the media center, in
 * the shapes those take, so the logic behind them stays testable without a browser.
 */
import type { BrowserInfo } from '@wirecatch/protocol';
import { browser } from 'wxt/browser';
import { detectBrowser, type Brand } from '../browser-info';
import { memoryArea, RETIRED_LOCAL_KEYS, storageCache } from '../cache';
import {
  cookieHeader,
  cookieReads,
  cookiesToSend,
  mergeCookies,
  type CookieContext,
  type CookieQuery,
  type CookieStore,
  type StoredCookie,
} from '../cookies';
import { fallback, log } from '../diag/log';
import type {
  Badge,
  BrowserDownloads,
  InFlightEntry,
  InFlightStore,
  NoticeStore,
  SettingsCache,
} from '../hub/deps';
import type { MediaStore, TabInfo } from '../media/center';
import { MEDIA_BUTTON_SETTING, type ButtonSetting } from '../media/messages';
import { isNotice } from '../messages';
import type { HelloIdentity } from '../native/client';
import { originPattern } from '../site-access';
import { fields, isNumber, isString, itemsOf, object } from '../validate';

const IN_FLIGHT_KEY = 'wirecatch.inFlight';
const NOTICE_KEY = 'wirecatch.notices';
const MEDIA_KEY = 'wirecatch.media.v1';

interface FirefoxRuntime {
  getBrowserInfo?: () => Promise<{ name: string; version: string }>;
}

interface NavigatorClues {
  userAgentData?: { brands?: Brand[] };
  brave?: unknown;
}

/** True in Firefox, which alone has `runtime.getBrowserInfo`. */
export function isFirefox(): boolean {
  const runtime: typeof browser.runtime & FirefoxRuntime = browser.runtime;
  return typeof runtime.getBrowserInfo === 'function';
}

async function browserInfo(): Promise<BrowserInfo> {
  const runtime: typeof browser.runtime & FirefoxRuntime = browser.runtime;
  const firefox = runtime.getBrowserInfo
    ? await runtime
        .getBrowserInfo()
        .catch(fallback('native', 'Could not read the browser name and version', null))
    : null;
  const nav: Navigator & NavigatorClues = navigator;
  return detectBrowser({
    firefox,
    brands: nav.userAgentData?.brands ?? null,
    isBrave: nav.brave !== undefined,
    userAgent: nav.userAgent,
  });
}

/** What this extension says about itself in `hello`. */
export async function helloFromExtension(): Promise<HelloIdentity> {
  return {
    client: 'extension',
    client_version: browser.runtime.getManifest().version,
    browser: await browserInfo(),
  };
}

/** The cookies the browser would send with the request, read the way this browser supports. */
export async function cookieHeaderFor(
  url: string,
  store: CookieStore,
  context: CookieContext,
): Promise<string | null> {
  const storeId = await storeToRead(store);
  if (storeId === undefined) {
    // The normal window's cookies would sign a private download in as someone else (D82).
    log.warn('capture', 'Could not find the cookies of the private windows; sending none');
    return null;
  }
  const queries = cookieReads(url, storeId, context);
  const reads = await Promise.all(queries.map(firstAccepted));
  const failures = reads.flatMap((read, i) =>
    'rejected' in read
      ? [
          {
            error: read.rejected,
            partitionOnly: queries[i]?.every((q) => q.partitionKey !== undefined),
          },
        ]
      : [],
  );
  if (failures.length === reads.length) {
    // Without cookies the app still tries; downloads that need a login then report it in the app.
    log.warn('capture', 'Could not read the cookies; sending none', failures.at(-1)?.error);
  } else {
    for (const { error, partitionOnly } of failures) {
      // Chromium before 119 knows no partitions, so a failed partitioned read is expected there.
      if (partitionOnly) log.debug('capture', 'Could not read the partitioned cookies', error);
      else log.warn('capture', 'Could not read some of the cookies', error);
    }
  }
  const cookies = reads.flatMap((read) => ('cookies' in read ? [read.cookies] : []));
  return cookieHeader(cookiesToSend(mergeCookies(cookies), url, context));
}

/** The store id to read, null for the default store; undefined when a private window's cannot be found. */
async function storeToRead(store: CookieStore): Promise<string | null | undefined> {
  if (store.id !== null || !store.private) return store.id;
  // Chromium names no store on a private tab or download: it is the store holding the private tabs.
  try {
    const [stores, tabs] = await Promise.all([
      browser.cookies.getAllCookieStores(),
      browser.tabs.query({}),
    ]);
    const privateTabs = new Set(tabs.filter((tab) => tab.incognito).map((tab) => tab.id));
    return stores.find((s) => s.tabIds.some((id) => privateTabs.has(id)))?.id;
  } catch (error) {
    log.warn('capture', 'Could not read the cookie stores', error);
    return undefined;
  }
}

/** The cookies of the first query this browser accepts, or why it rejected the last. */
async function firstAccepted(
  queries: CookieQuery[],
): Promise<{ cookies: StoredCookie[] } | { rejected: unknown }> {
  let rejected: unknown = null;
  for (const query of queries) {
    try {
      return { cookies: await browser.cookies.getAll(query) };
    } catch (error) {
      // This browser rejects the query (first-party isolation, or no partitioned cookies): try the next.
      rejected = error;
    }
  }
  return { rejected };
}

export async function hasSiteAccess(url: string): Promise<boolean> {
  const pattern = originPattern(url);
  if (pattern === null) return false;
  return browser.permissions.contains({ origins: [pattern] });
}

export function browserDownloads(): BrowserDownloads {
  return {
    pause: (id) => browser.downloads.pause(id),
    resume: (id) => browser.downloads.resume(id),
    cancel: (id) => browser.downloads.cancel(id),
    erase: async (id) => {
      await browser.downloads.erase({ id });
    },
    state: async (id) => {
      const [item] = await browser.downloads.search({ id });
      return item === undefined
        ? null
        : { state: item.state, paused: item.paused, danger: item.danger, filename: item.filename };
    },
    start: async (url) => {
      await browser.downloads.download({ url });
    },
  };
}

/** `storage.session`, which survives a restarted background script but not a browser restart; missing in old browsers. */
export function sessionArea(): typeof browser.storage.session | undefined {
  return (browser.storage as Partial<typeof browser.storage>).session;
}

/** The copy of the app's settings for this browser session (D61), in `storage.session`, or in memory where the browser has none. */
export function settingsCache(): SettingsCache {
  return storageCache(sessionArea() ?? memoryArea());
}

/** Removes what earlier versions kept in `storage.local`, so nothing of the app's settings outlives the browser session (D61). */
export function retireLocalSettings(): void {
  browser.storage.local
    .remove([...RETIRED_LOCAL_KEYS])
    .catch(
      fallback('settings', 'Could not remove the settings an earlier version kept', undefined),
    );
}

/** Tells the media button on every open page that "Show download button on videos" changed. */
export async function publishButtonSetting(on: boolean): Promise<void> {
  const message: ButtonSetting = { type: MEDIA_BUTTON_SETTING, on };
  const tabs = await browser.tabs.query({});
  await Promise.all(
    tabs.map(async ({ id }) => {
      if (id === undefined) return;
      await browser.tabs.sendMessage(id, message).catch(() => {
        // Expected: pages without the button (browser pages, other extensions) cannot take it.
      });
    }),
  );
}

const isInFlightEntry = fields<InFlightEntry>({ id: isNumber, handoverId: isString });

export function inFlightStore(): InFlightStore | undefined {
  const area = sessionArea();
  if (area === undefined) return undefined;
  return {
    async load() {
      const items = await area.get(IN_FLIGHT_KEY);
      return itemsOf(items[IN_FLIGHT_KEY], isInFlightEntry);
    },
    async save(entries) {
      await area.set({ [IN_FLIGHT_KEY]: entries });
    },
  };
}

export function noticeStore(): NoticeStore | undefined {
  const area = sessionArea();
  if (area === undefined) return undefined;
  return {
    async load() {
      const items = await area.get(NOTICE_KEY);
      return itemsOf(items[NOTICE_KEY], isNotice);
    },
    async save(notices) {
      await area.set({ [NOTICE_KEY]: notices });
    },
  };
}

export function mediaStore(): MediaStore | undefined {
  const area = sessionArea();
  if (area === undefined) return undefined;
  return {
    async load() {
      const items = await area.get(MEDIA_KEY);
      return items[MEDIA_KEY];
    },
    async save(value) {
      await area.set({ [MEDIA_KEY]: value });
    },
  };
}

const hasCookieStore = object({ cookieStoreId: isString });

/** The Firefox container or private cookie store of a tab; null in Chromium, whose tabs name none. */
export function cookieStoreIdOf(tab: unknown): string | null {
  return hasCookieStore(tab) ? tab.cookieStoreId : null;
}

export async function tabInfo(tabId: number): Promise<TabInfo | null> {
  try {
    const tab = await browser.tabs.get(tabId);
    return {
      url: tab.url ?? null,
      title: tab.title ?? null,
      incognito: tab.incognito,
      cookieStoreId: cookieStoreIdOf(tab),
    };
  } catch (error) {
    // The tab closed meanwhile, or is one the extension cannot see.
    log.debug('media', 'Could not read a tab', error);
    return null;
  }
}

/** Every open tab with the address it shows (null when it cannot be read). */
export async function liveTabs(): Promise<{ id: number; url: string | null }[]> {
  const tabs = await browser.tabs.query({});
  return tabs.flatMap((tab) =>
    tab.id === undefined ? [] : [{ id: tab.id, url: tab.url ?? null }],
  );
}

export function toolbarBadge(): Badge {
  let last = '';
  return {
    set(text, title) {
      const key = `${text}\n${title}`;
      if (key === last) return;
      last = key;
      const failed = fallback('hub', 'Could not update the toolbar button', undefined);
      void browser.action.setBadgeText({ text }).catch(failed);
      void browser.action.setTitle({ title }).catch(failed);
      if (text !== '') {
        void browser.action.setBadgeBackgroundColor({ color: __BADGE_COLOR__ }).catch(failed);
      }
    },
  };
}
