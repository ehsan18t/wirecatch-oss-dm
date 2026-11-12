import { PRODUCT_NAME } from '@wirecatch/product';
import { browser } from 'wxt/browser';
import { defineBackground } from 'wxt/utils/define-background';
import {
  browserDownloads,
  cookiesFor,
  cookieStoreIdOf,
  hasSiteAccess,
  helloFromExtension,
  inFlightStore,
  isFirefox,
  liveTabs,
  mediaStore,
  alertStore,
  publishButtonSetting,
  retireLocalSettings,
  settingsCache,
  tabInfo,
  toolbarBadge,
} from '@/lib/browser/adapters';
import { registerCapture, type DownloadEvents } from '@/lib/capture/register';
import { BrowserRoutes, type FileRequest } from '@/lib/capture/browser-route';
import { installDiagnostics } from '@/lib/diag/install';
import { fallback } from '@/lib/diag/log';
import { appSide } from '@/lib/hub/app';
import type { HubDeps, MenuTab } from '@/lib/hub/deps';
import { Hub } from '@/lib/hub/hub';
import { MediaSender } from '@/lib/hub/media-sender';
import { announcer } from '@/lib/media/announce';
import { MediaCenter } from '@/lib/media/center';
import { registerMediaListeners } from '@/lib/media/listen';
import { MediaRegistry } from '@/lib/media/registry';
import { MediaWatcher } from '@/lib/media/watch';
import { acceptsPopupPort } from '@/lib/popup-port';
import { NativeClient } from '@/lib/native/client';
import { browserTransport } from '@/lib/native/port';
import { NoticeCenter } from '@/lib/notices/center';
import { focusedWebTab } from '@/lib/notices/focus';
import { NOTICE_MOUNT, NOTICE_UNMOUNT, NOTICES_PORT, isMountAnswer } from '@/lib/notices/messages';
import { SITE_ORIGINS } from '@/lib/site-access';
import { parseWebUrl } from '@/lib/url';

const MENU_ID = 'wirecatch-download';

/** "Download with Wirecatch" on links and media, created once per install or update. */
function registerContextMenu(hub: Hub): void {
  browser.runtime.onInstalled.addListener(() => {
    browser.contextMenus
      .removeAll()
      .then(() => {
        browser.contextMenus.create({
          id: MENU_ID,
          title: `Download with ${PRODUCT_NAME}`,
          contexts: ['link', 'image', 'video', 'audio'],
        });
      })
      .catch(fallback('background', 'Could not add the context menu item', undefined, 'error'));
  });
  browser.contextMenus.onClicked.addListener((info, tab) => {
    if (info.menuItemId !== MENU_ID) return;
    const menuTab: MenuTab | undefined =
      tab === undefined
        ? undefined
        : { incognito: tab.incognito, cookieStoreId: cookieStoreIdOf(tab) };
    hub
      .onMenuClick(info, menuTab)
      .catch(fallback('hub', 'The context menu download failed', undefined, 'error'));
  });
}

/** Watches responses and tabs for media, and answers the media button and its keyboard shortcut. */
function registerMedia(hub: Hub, media: MediaCenter, watcher: MediaWatcher): void {
  registerMediaListeners(
    {
      onSendHeaders: browser.webRequest.onSendHeaders,
      onResponseStarted: browser.webRequest.onResponseStarted,
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
      announce: announcer({
        sendMessage: (tabId, message, options) => browser.tabs.sendMessage(tabId, message, options),
        notify: (tone, message) => hub.notify(tone, message),
      }),
    },
  );
}

/** A newly opened tab may still be blank when its download begins. */
async function downloadTab(tabId: number) {
  const tab = await browser.tabs.get(tabId);
  if (tab.openerTabId === undefined || parseWebUrl(tab.url ?? '') !== null) return tab;
  const opener = await browser.tabs
    .get(tab.openerTabId)
    .catch(fallback('capture', 'Could not read the tab that opened the download', null));
  return opener?.incognito === tab.incognito ? opener : tab;
}

/** Remembers the requests that can become downloads, and the navigation that led to each. */
function registerRoutes(routes: BrowserRoutes): void {
  const files = {
    urls: SITE_ORIGINS,
    types: ['main_frame', 'sub_frame', 'other'],
  } satisfies Parameters<typeof browser.webRequest.onHeadersReceived.addListener>[1];
  const seen = (details: FileRequest) => {
    routes.seen(details);
    return undefined;
  };
  browser.webRequest.onBeforeRequest.addListener(seen, files);
  browser.webRequest.onHeadersReceived.addListener(seen, files);
  browser.webRequest.onBeforeRedirect.addListener((details) => routes.redirect(details), files);
}

export default defineBackground(() => {
  installDiagnostics('background', self);
  const client = new NativeClient({ transport: browserTransport(), hello: helloFromExtension });

  const hubDeps = {
    mediaButtonSetting: { publish: publishButtonSetting },
    client,
    cache: settingsCache(),
    downloads: browserDownloads(),
    cookies: cookiesFor,
    hasSiteAccess,
    userAgent: navigator.userAgent,
    inFlight: inFlightStore(),
    alert: alertStore(),
    badge: toolbarBadge(),
  } satisfies HubDeps;
  // Built before the hub, the media side and the notices, which all use them.
  const side = appSide(hubDeps);
  const { settings, app } = side;
  const sender = new MediaSender(side, hubDeps);

  const registry = new MediaRegistry();
  const media = new MediaCenter({
    registry,
    getTab: tabInfo,
    send: (request) => sender.send(request),
    store: mediaStore(),
    ignoredSites: () => settings.known()?.ignored_sites ?? [],
    videoSites: () => settings.known()?.video_sites ?? [],
    mediaButtonOn: () => settings.mediaButtonOnceLoaded(),
    privateWindowsOn: () => app.privateWindowsOn(),
    liveTabs,
  });
  // Started before any listener can fire, so every response waits for the restored lists.
  const restored = media.start();
  const watcher = new MediaWatcher(registry, {
    isPrivateTab: async (tabId) => (await browser.tabs.get(tabId)).incognito,
    privateWindowsOn: () => app.privateWindowsOn(),
    ready: restored,
  });

  const notices = new NoticeCenter({
    focusedTab: () =>
      focusedWebTab({
        lastFocusedWindow: () => browser.windows.getLastFocused(),
        activeTab: async (windowId) => (await browser.tabs.query({ active: true, windowId }))[0],
        privateWindowsOn: () => app.privateWindowsOn(),
      }),
    mount: async (tabId) => {
      const answer: unknown = await browser.tabs.sendMessage(
        tabId,
        { type: NOTICE_MOUNT },
        { frameId: 0 },
      );
      return isMountAnswer(answer) ? answer.token : null;
    },
    unmount: async (tabId) => {
      await browser.tabs.sendMessage(tabId, { type: NOTICE_UNMOUNT }, { frameId: 0 });
    },
    shown: (id) => app.noticeShown(id),
    pressed: (id, button) => app.noticePressed(id, button),
    now: () => Date.now(),
  });

  const routes = new BrowserRoutes((tabId) =>
    downloadTab(tabId).catch(fallback('capture', 'Could not read the source tab', null)),
  );
  const hub = new Hub({ ...hubDeps, media, notices, routes }, side);

  // Every listener is registered synchronously so a sleeping background wakes up for it.
  const downloadEvents: DownloadEvents = browser.downloads;
  registerCapture(downloadEvents, hub);
  registerRoutes(routes);
  registerContextMenu(hub);
  registerMedia(hub, media, watcher);
  browser.runtime.onConnect.addListener((port) => {
    // The notices page proves itself with its frame's token; nothing else reaches the notices.
    if (port.name === NOTICES_PORT) {
      notices.connect(port);
      return;
    }
    // Only the extension's own popup, never a content script that knows the port name.
    if (!acceptsPopupPort(port, browser.runtime.id, browser.runtime.getURL('/'))) return;
    hub.addPopup(port);
  });
  browser.tabs.onRemoved.addListener((tabId) => notices.tabRemoved(tabId));
  browser.tabs.onActivated.addListener(() => void notices.focusChanged());
  browser.windows.onFocusChanged.addListener(() => void notices.focusChanged());
  // A page that loaded took the old page's frame with it; the open notices show on the new one.
  browser.tabs.onUpdated.addListener((_tabId, change) => {
    if (change.status === 'complete') void notices.focusChanged();
  });

  browser.runtime.onInstalled.addListener(retireLocalSettings);
  void hub.start();
});
