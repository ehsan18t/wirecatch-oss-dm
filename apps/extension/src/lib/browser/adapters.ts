/**
 * The browser APIs the background script hands to the hub and the media center, in
 * the shapes those take, so the logic behind them stays testable without a browser.
 */
import { PROTOCOL_VERSION, type BrowserInfo, type Hello } from '@wirecatch/protocol';
import { browser } from 'wxt/browser';
import { detectBrowser, type Brand } from '../browser-info';
import { cookieHeader, cookieQueries, cookiesToSend, type CookieContext } from '../cookies';
import type { Badge, BrowserDownloads, InFlightStore } from '../hub/deps';
import type { MediaStore, TabInfo } from '../media/center';
import { originPattern } from '../site-access';
import { isNumber, itemsOf } from '../validate';

const IN_FLIGHT_KEY = 'wirecatch.inFlight';
const MEDIA_KEY = 'wirecatch.media.v1';

/** The badge's background while it shows a message: the popup's `--warn` color (`popup/style.css`). */
const BADGE_COLOR = '#b45309';

interface FirefoxRuntime {
  getBrowserInfo?: () => Promise<{ name: string; version: string }>;
}

interface NavigatorClues {
  userAgentData?: { brands?: Brand[] };
  brave?: unknown;
}

/** True in Firefox, which alone has `runtime.getBrowserInfo`. */
export function isFirefox(): boolean {
  return typeof (browser.runtime as FirefoxRuntime).getBrowserInfo === 'function';
}

async function browserInfo(): Promise<BrowserInfo> {
  const runtime = browser.runtime as typeof browser.runtime & FirefoxRuntime;
  const firefox = runtime.getBrowserInfo ? await runtime.getBrowserInfo().catch(() => null) : null;
  const nav: Navigator & NavigatorClues = navigator;
  return detectBrowser({
    firefox,
    brands: nav.userAgentData?.brands ?? null,
    isBrave: nav.brave !== undefined,
    userAgent: nav.userAgent,
  });
}

/** What this extension says about itself in `hello`. */
export async function helloFromExtension(): Promise<Omit<Hello, 'start_app'>> {
  return {
    client: 'extension',
    client_version: browser.runtime.getManifest().version,
    protocol_version: PROTOCOL_VERSION,
    browser: await browserInfo(),
  };
}

/** The cookies the browser would send with the request, read the way this browser supports. */
export async function cookieHeaderFor(
  url: string,
  storeId: string | null,
  context: CookieContext,
): Promise<string | null> {
  for (const query of cookieQueries(url, storeId, context)) {
    try {
      const cookies = await browser.cookies.getAll(query);
      return cookieHeader(cookiesToSend(cookies, url, context));
    } catch {
      // This browser rejects the query (first-party isolation, or no partitioned cookies): try the next.
    }
  }
  // Without cookies the app still tries; downloads that need a login then report it in the app.
  return null;
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
        : { state: item.state, paused: item.paused, danger: item.danger };
    },
    start: async (url) => {
      await browser.downloads.download({ url });
    },
  };
}

/** `storage.session`, which survives a restarted background script but not a browser restart; missing in old browsers. */
function sessionArea(): typeof browser.storage.session | undefined {
  return (browser.storage as Partial<typeof browser.storage>).session;
}

export function inFlightStore(): InFlightStore | undefined {
  const area = sessionArea();
  if (area === undefined) return undefined;
  return {
    async load() {
      const items = await area.get(IN_FLIGHT_KEY);
      return itemsOf(items[IN_FLIGHT_KEY], isNumber);
    },
    async save(ids) {
      await area.set({ [IN_FLIGHT_KEY]: ids });
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

export async function tabInfo(tabId: number): Promise<TabInfo | null> {
  try {
    const tab = await browser.tabs.get(tabId);
    // Firefox adds `cookieStoreId` to tabs; Chromium tabs use the default cookie store.
    const cookieStoreId = (tab as { cookieStoreId?: string }).cookieStoreId ?? null;
    return {
      url: tab.url ?? null,
      title: tab.title ?? null,
      incognito: tab.incognito,
      cookieStoreId,
    };
  } catch {
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
      void browser.action.setBadgeText({ text }).catch(() => undefined);
      void browser.action.setTitle({ title }).catch(() => undefined);
      if (text !== '') {
        void browser.action.setBadgeBackgroundColor({ color: BADGE_COLOR }).catch(() => undefined);
      }
    },
  };
}
