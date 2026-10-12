import { browser } from 'wxt/browser';
import { defineBackground } from 'wxt/utils/define-background';
import {
  browserDownloads,
  cookieHeaderFor,
  hasSiteAccess,
  helloFromExtension,
  inFlightStore,
  isFirefox,
  liveTabs,
  mediaStore,
  tabInfo,
  toolbarBadge,
} from '@/lib/browser/adapters';
import { storageCache } from '@/lib/cache';
import { registerCapture, type DownloadEvents } from '@/lib/capture/register';
import type { MenuTab } from '@/lib/hub/deps';
import { Hub } from '@/lib/hub/hub';
import { MediaCenter, type MediaSendResult } from '@/lib/media/center';
import { registerMediaListeners, type MediaEventApi } from '@/lib/media/listen';
import { MEDIA_ANNOUNCE, MEDIA_BUTTON_KEY, type AnnounceMessage } from '@/lib/media/messages';
import { MediaRegistry } from '@/lib/media/registry';
import { MediaWatcher } from '@/lib/media/watch';
import { acceptsPopupPort } from '@/lib/messages';
import { NativeClient } from '@/lib/native/client';
import { browserTransport } from '@/lib/native/port';

const MENU_ID = 'wirecatch-download';

/** "Download with Wirecatch" on links and media, created once per install or update. */
function registerContextMenu(hub: Hub): void {
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
}

/** Watches responses and tabs for media, and answers the media button and its keyboard shortcut. */
function registerMedia(hub: Hub, media: MediaCenter, watcher: MediaWatcher): void {
  registerMediaListeners(
    {
      onResponseStarted: browser.webRequest
        .onResponseStarted as unknown as MediaEventApi['onResponseStarted'],
      onTabRemoved: browser.tabs.onRemoved,
      onTabUpdated: browser.tabs.onUpdated,
      onMessage: browser.runtime.onMessage,
      onCommand: browser.commands.onCommand,
      extensionId: browser.runtime.id,
      firefox: isFirefox(),
    },
    {
      watcher,
      media,
      announce: (tabId, result) => {
        // The page's top frame shows it next to where the person is looking; pages without
        // the content script (browser pages, other extensions) get it in the popup instead.
        const message: AnnounceMessage = {
          type: MEDIA_ANNOUNCE,
          ok: result.ok,
          message: result.message,
        };
        browser.tabs.sendMessage(tabId, message, { frameId: 0 }).catch(() => {
          hub.notify(result.ok ? 'warning' : 'error', result.message);
        });
      },
    },
  );
}

export default defineBackground(() => {
  const client = new NativeClient({ transport: browserTransport(), hello: helloFromExtension });

  const registry = new MediaRegistry();
  const media: MediaCenter = new MediaCenter({
    registry,
    getTab: tabInfo,
    send: (request): Promise<MediaSendResult> => hub.sendMedia(request),
    store: mediaStore(),
    ignoredSites: () => hub.knownSettings()?.ignored_sites ?? [],
    liveTabs,
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
    downloads: browserDownloads(),
    cookieHeader: cookieHeaderFor,
    hasSiteAccess,
    userAgent: navigator.userAgent,
    inFlight: inFlightStore(),
    badge: toolbarBadge(),
  });

  // Every listener is registered synchronously so a sleeping background wakes up for it.
  const downloadEvents: DownloadEvents = browser.downloads;
  registerCapture(downloadEvents, hub);
  registerContextMenu(hub);
  registerMedia(hub, media, watcher);
  browser.runtime.onConnect.addListener((port) => {
    // Only the popup itself, never a content script that knows the port name.
    if (!acceptsPopupPort(port, browser.runtime.id, browser.runtime.getURL('/'))) return;
    hub.addPopup(port);
  });

  void hub.start();
});
