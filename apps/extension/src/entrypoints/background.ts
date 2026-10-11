import { PROTOCOL_VERSION, type BrowserInfo, type Hello } from '@wirecatch/protocol';
import { browser } from 'wxt/browser';
import { defineBackground } from 'wxt/utils/define-background';
import { detectBrowser, type Brand } from '@/lib/browser-info';
import { storageCache } from '@/lib/cache';
import { registerCapture, type DownloadEvents } from '@/lib/capture/register';
import { cookieHeader, cookieQueries, cookiesToSend, type CookieContext } from '@/lib/cookies';
import { Hub, type Badge, type BrowserDownloads, type InFlightStore, type MenuTab } from '@/lib/hub';
import { MediaCenter, type MediaSendResult, type MediaStore, type TabInfo } from '@/lib/media/center';
import { registerMediaListeners, type MediaEventApi } from '@/lib/media/listen';
import { MEDIA_ANNOUNCE, MEDIA_BUTTON_KEY, type AnnounceMessage } from '@/lib/media/messages';
import { MediaRegistry } from '@/lib/media/registry';
import { MediaWatcher } from '@/lib/media/watch';
import { acceptsPopupPort } from '@/lib/messages';
import { NativeClient, type NativePort } from '@/lib/native/client';
import { disconnectMessage } from '@/lib/native/status';
import { originPattern } from '@/lib/site-access';

const MENU_ID = 'wirecatch-download';
const IN_FLIGHT_KEY = 'wirecatch.inFlight';
const MEDIA_KEY = 'wirecatch.media.v1';

interface FirefoxRuntime {
  getBrowserInfo?: () => Promise<{ name: string; version: string }>;
}

interface NavigatorClues {
  userAgentData?: { brands?: Brand[] };
  brave?: unknown;
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

/** The cookies the browser would send with the request, read the way this browser supports. */
async function cookieHeaderFor(
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

async function hasSiteAccess(url: string): Promise<boolean> {
  const pattern = originPattern(url);
  if (pattern === null) return false;
  return browser.permissions.contains({ origins: [pattern] });
}

function inFlightStore(): InFlightStore | undefined {
  // storage.session survives a restarted background script but not a browser restart.
  const area = (browser.storage as Partial<typeof browser.storage>).session;
  if (area === undefined) return undefined;
  return {
    async load() {
      const items = await area.get(IN_FLIGHT_KEY);
      const ids: unknown = items[IN_FLIGHT_KEY];
      return Array.isArray(ids) ? ids.filter((id): id is number => typeof id === 'number') : [];
    },
    async save(ids) {
      await area.set({ [IN_FLIGHT_KEY]: ids });
    },
  };
}

function mediaStore(): MediaStore | undefined {
  const area = (browser.storage as Partial<typeof browser.storage>).session;
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

async function tabInfo(tabId: number): Promise<TabInfo | null> {
  try {
    const tab = await browser.tabs.get(tabId);
    // Firefox adds `cookieStoreId` to tabs; Chromium tabs use the default cookie store.
    const cookieStoreId = (tab as { cookieStoreId?: string }).cookieStoreId ?? null;
    return { url: tab.url ?? null, title: tab.title ?? null, incognito: tab.incognito, cookieStoreId };
  } catch {
    return null;
  }
}

function makeBadge(): Badge {
  let last = '';
  return {
    set(text, title) {
      const key = `${text}\n${title}`;
      if (key === last) return;
      last = key;
      void browser.action.setBadgeText({ text }).catch(() => undefined);
      void browser.action.setTitle({ title }).catch(() => undefined);
      if (text !== '') {
        void browser.action.setBadgeBackgroundColor({ color: '#b45309' }).catch(() => undefined);
      }
    },
  };
}

export default defineBackground(() => {
  const client = new NativeClient({
    transport: {
      connect: (): NativePort => browser.runtime.connectNative(__NATIVE_HOST_NAME__),
      disconnectReason: (port) => disconnectMessage(browser.runtime.lastError, port),
    },
    hello: async (): Promise<Hello> => ({
      client: 'extension',
      client_version: browser.runtime.getManifest().version,
      protocol_version: PROTOCOL_VERSION,
      browser: await browserInfo(),
    }),
  });

  const downloads: BrowserDownloads = {
    pause: (id) => browser.downloads.pause(id),
    resume: (id) => browser.downloads.resume(id),
    cancel: (id) => browser.downloads.cancel(id),
    erase: async (id) => {
      await browser.downloads.erase({ id });
    },
    state: async (id) => {
      const [item] = await browser.downloads.search({ id });
      return item === undefined ? null : { state: item.state, paused: item.paused, danger: item.danger };
    },
    start: async (url) => {
      await browser.downloads.download({ url });
    },
  };

  const registry = new MediaRegistry();
  const media: MediaCenter = new MediaCenter({
    registry,
    getTab: tabInfo,
    send: (request): Promise<MediaSendResult> => hub.sendMedia(request),
    store: mediaStore(),
    ignoredSites: () => hub.knownSettings()?.ignored_sites ?? [],
    liveTabIds: async () =>
      (await browser.tabs.query({})).flatMap((tab) => (tab.id === undefined ? [] : [tab.id])),
  });
  // Started before any listener can fire, so every response waits for the restored lists.
  const restored = media.start();
  const watcher = new MediaWatcher(registry, {
    isPrivateTab: async (tabId) => (await browser.tabs.get(tabId)).incognito,
    ready: restored,
  });

  const hub: Hub = new Hub({
    media,
    mediaButtonSetting: {
      save: (on) => browser.storage.local.set({ [MEDIA_BUTTON_KEY]: on }),
    },
    client,
    cache: storageCache(browser.storage.local),
    downloads,
    cookieHeader: cookieHeaderFor,
    hasSiteAccess,
    userAgent: navigator.userAgent,
    inFlight: inFlightStore(),
    badge: makeBadge(),
  });

  // Every listener is registered synchronously so a sleeping background wakes up for it.
  const downloadEvents: DownloadEvents = browser.downloads;
  registerCapture(downloadEvents, hub);

  browser.runtime.onInstalled.addListener(() => {
    void browser.contextMenus.removeAll().then(() => {
      browser.contextMenus.create({
        id: MENU_ID,
        title: 'Download with Wirecatch',
        contexts: ['link', 'image', 'video', 'audio'],
      });
    });
  });
  browser.contextMenus.onClicked.addListener((info, tab) => {
    if (info.menuItemId !== MENU_ID) return;
    // Firefox adds `cookieStoreId` to tabs; Chromium tabs use the default cookie store.
    const menuTab = tab as MenuTab | undefined;
    hub.onMenuClick(info, menuTab).catch(() => undefined);
  });

  registerMediaListeners(
    {
      onResponseStarted: browser.webRequest
        .onResponseStarted as unknown as MediaEventApi['onResponseStarted'],
      onTabRemoved: browser.tabs.onRemoved,
      onTabUpdated: browser.tabs.onUpdated,
      onMessage: browser.runtime.onMessage,
      onCommand: browser.commands.onCommand,
      extensionId: browser.runtime.id,
      firefox: typeof (browser.runtime as { getBrowserInfo?: unknown }).getBrowserInfo === 'function',
    },
    {
      watcher,
      media,
      announce: (tabId, result) => {
        // The page's top frame shows it next to where the person is looking; pages without
        // the content script (browser pages, other extensions) get it in the popup instead.
        const message: AnnounceMessage = { type: MEDIA_ANNOUNCE, ok: result.ok, message: result.message };
        browser.tabs.sendMessage(tabId, message, { frameId: 0 }).catch(() => {
          hub.notify(result.ok ? 'warning' : 'error', result.message);
        });
      },
    },
  );

  browser.runtime.onConnect.addListener((port) => {
    // Only the popup itself, never a content script that knows the port name.
    if (!acceptsPopupPort(port, browser.runtime.id, browser.runtime.getURL('/'))) return;
    hub.addPopup(port);
  });

  void hub.start();
});
