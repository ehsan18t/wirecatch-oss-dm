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
import {
  BrowserRoutes,
  SourceTabs,
  type FileRequest,
  type SourceTab,
} from '@/lib/capture/browser-route';
import { installDiagnostics } from '@/lib/diag/install';
import { fallback } from '@/lib/diag/log';
import { appSide, type AppSide } from '@/lib/hub/app';
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
import { browserProfile } from '@/lib/browser-profile';
import { PageReads } from '@/lib/page-reads';
import { ReachedAddresses } from '@/lib/page-read-addresses';
import { browserTransport } from '@/lib/native/port';
import { NoticeCenter } from '@/lib/notices/center';
import { focusedWebTab } from '@/lib/notices/focus';
import { NOTICE_MOUNT, NOTICE_UNMOUNT, NOTICES_PORT, isMountAnswer } from '@/lib/notices/messages';
import { SITE_ORIGINS } from '@/lib/site-access';
import { PrivateSession } from '@/lib/private-session';
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
async function downloadTab(tabId: number): Promise<SourceTab> {
  const tab = await browser.tabs.get(tabId);
  if (tab.openerTabId === undefined || parseWebUrl(tab.url ?? '') !== null) return tab;
  const opener = await browser.tabs
    .get(tab.openerTabId)
    .catch(fallback('capture', 'Could not read the tab that opened the download', null));
  return opener?.incognito === tab.incognito
    ? { url: opener.url, incognito: opener.incognito, lent: true }
    : tab;
}

/** Remembers the requests that can become downloads, and the navigation that led to each. */
function watchedRoutes(): BrowserRoutes {
  const tabs = new SourceTabs((tabId) =>
    downloadTab(tabId).catch(fallback('capture', 'Could not read the source tab', null)),
  );
  // A navigation that starts also counts: a tab the extension may not read reports no new address.
  browser.tabs.onUpdated.addListener((tabId, change) => {
    if (change.url !== undefined || change.status === 'loading') tabs.forget(tabId);
  });
  browser.tabs.onRemoved.addListener((tabId) => tabs.forget(tabId));
  const routes = new BrowserRoutes((tabId) => tabs.of(tabId));
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
  return routes;
}

function registerDownloads(hub: Hub): void {
  const downloadEvents: DownloadEvents = browser.downloads;
  registerCapture(downloadEvents, hub);
}

/** Shows the app's notices in the tab the person is looking at (D65). Unasked work: it reads only the values held. */
function noticeCenter({ settings, app }: AppSide): NoticeCenter {
  return new NoticeCenter({
    focusedTab: () =>
      focusedWebTab({
        lastFocusedWindow: () => browser.windows.getLastFocused(),
        activeTab: async (windowId) => (await browser.tabs.query({ active: true, windowId }))[0],
        privateWindowsOn: () => settings.privateWindowsKnownOn(),
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
}

/** The browser's private session (D280), told when its last private window closes; `ended` is set once the app side exists. */
function followPrivateSession(): {
  session: PrivateSession;
  ended: { tell: (id: string) => void };
} {
  const ended = { tell: (_id: string): void => undefined };
  const session = new PrivateSession({
    area: browser.storage.local,
    anyPrivateWindow: async () => (await browser.windows.getAll()).some((w) => w.incognito),
    newId: () => crypto.randomUUID(),
    ended: (id) => {
      ended.tell(id);
    },
  });
  browser.windows.onRemoved.addListener(() => void session.windowsChanged());
  return { session, ended };
}

/** This browser profile's id (D282), read or made once, and read again at the next hello after a storage failure. */
function keptProfile(): () => Promise<string> {
  let kept: Promise<string> | null = null;
  return () =>
    (kept ??= browserProfile(browser.storage.local, () => crypto.randomUUID()).catch(
      (error: unknown) => {
        kept = null;
        throw error;
      },
    ));
}

/** Makes the app's page reads (D282), telling it the address each of its connections reached. */
function servePageReads(client: NativeClient): void {
  const reached = new ReachedAddresses(new URL(browser.runtime.getURL('/')).origin, Date.now);
  browser.webRequest.onResponseStarted.addListener((response) => reached.seen(response), {
    urls: ['<all_urls>'],
  });
  const pageReads = new PageReads({
    fetch: (url, init) => fetch(url, init),
    send: (call, timeoutMs) => client.request(call, timeoutMs),
    addressOf: (url, since) => reached.of(url, since),
    now: Date.now,
  });
  client.onEvent((event) => pageReads.follow(event));
  client.onStatus((status) => {
    if (status.state !== 'connected') pageReads.disconnected();
  });
}

export default defineBackground(() => {
  installDiagnostics('background', self);
  const { session: privateSession, ended: sessionEnded } = followPrivateSession();
  // Firefox's fetch() from an extension has not been measured against a site's checks yet (D282).
  const makesPageReads = !isFirefox();
  const profile = makesPageReads ? keptProfile() : null;
  const client = new NativeClient({
    transport: browserTransport(),
    hello: () => helloFromExtension(privateSession, profile),
  });
  if (makesPageReads) servePageReads(client);

  const hubDeps = {
    mediaButtonSetting: { publish: publishButtonSetting },
    client,
    cache: settingsCache(),
    downloads: browserDownloads(),
    cookies: cookiesFor,
    privateSession,
    hasSiteAccess,
    userAgent: navigator.userAgent,
    inFlight: inFlightStore(),
    alert: alertStore(),
    badge: toolbarBadge(),
  } satisfies HubDeps;
  // Built before the hub, the media side and the notices, which all use them.
  const side = appSide(hubDeps);
  const { settings, app } = side;
  sessionEnded.tell = (id) => {
    app
      .privateSessionEnded(id)
      .catch(fallback('hub', 'Could not tell the app the private session ended', undefined));
  };
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
    // Unasked work reads only the values held: a page never starts the app (D25.6, D280).
    privateWindowsOn: () => settings.privateWindowsKnownOn(),
    liveTabs,
  });
  // Started before any listener can fire, so every response waits for the restored lists.
  const restored = media.start();
  const watcher = new MediaWatcher(registry, {
    isPrivateTab: async (tabId) => (await browser.tabs.get(tabId)).incognito,
    privateWindowsOn: () => settings.privateWindowsKnownOn(),
    privateWindowsNow: () => settings.known()?.private_windows === true,
    mediaTables: () => settings.known()?.media ?? null,
    ready: restored,
  });

  const notices = noticeCenter(side);

  // Every listener is registered synchronously so a sleeping background wakes up for it.
  const hub = new Hub({ ...hubDeps, media, notices, routes: watchedRoutes() }, side);
  registerDownloads(hub);
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
