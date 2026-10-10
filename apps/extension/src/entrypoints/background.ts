import { PROTOCOL_VERSION, type BrowserInfo, type Hello } from '@wirecatch/protocol';
import { browser } from 'wxt/browser';
import { defineBackground } from 'wxt/utils/define-background';
import { detectBrowser, type Brand } from '@/lib/browser-info';
import { storageCache } from '@/lib/cache';
import { registerCapture, type DownloadEvents } from '@/lib/capture/register';
import { cookieHeader } from '@/lib/cookies';
import { Hub, type Badge, type BrowserDownloads, type InFlightStore, type MenuTab } from '@/lib/hub';
import { POPUP_PORT_NAME } from '@/lib/messages';
import { NativeClient, type NativePort } from '@/lib/native/client';
import { originPattern } from '@/lib/site-access';

const MENU_ID = 'wirecatch-download';
const IN_FLIGHT_KEY = 'wirecatch.inFlight';

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

async function cookieHeaderFor(url: string, storeId: string | null): Promise<string | null> {
  try {
    const cookies = await browser.cookies.getAll(storeId === null ? { url } : { url, storeId });
    return cookieHeader(cookies);
  } catch {
    // Without cookies the app still tries; downloads that need a login then report it in the app.
    return null;
  }
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
      disconnectReason: (port) => {
        const firefoxError = (port as { error?: { message?: string } | null }).error?.message;
        return browser.runtime.lastError?.message ?? firefoxError ?? null;
      },
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
      return item === undefined ? null : { state: item.state, paused: item.paused };
    },
    start: async (url) => {
      await browser.downloads.download({ url });
    },
  };

  const hub = new Hub({
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

  browser.runtime.onConnect.addListener((port) => {
    if (port.name !== POPUP_PORT_NAME || port.sender?.id !== browser.runtime.id) return;
    hub.addPopup(port);
  });

  void hub.start();
});
