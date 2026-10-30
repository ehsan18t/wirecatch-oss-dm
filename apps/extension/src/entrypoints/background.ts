import { browser } from 'wxt/browser';
import { defineBackground } from 'wxt/utils/define-background';
import {
  browserDownloads,
  cookieHeaderFor,
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
import { installDiagnostics } from '@/lib/diag/install';
import { fallback, log } from '@/lib/diag/log';
import type { MenuTab } from '@/lib/hub/deps';
import { Hub } from '@/lib/hub/hub';
import { MediaCenter, type MediaSendResult } from '@/lib/media/center';
import { registerMediaListeners } from '@/lib/media/listen';
import { MEDIA_ANNOUNCE, type AnnounceMessage } from '@/lib/media/messages';
import { MediaRegistry } from '@/lib/media/registry';
import { MediaWatcher } from '@/lib/media/watch';
import { acceptsPopupPort } from '@/lib/messages';
import { NativeClient } from '@/lib/native/client';
import { browserTransport } from '@/lib/native/port';
import { NoticeCenter } from '@/lib/notices/center';
import { focusedWebTab } from '@/lib/notices/focus';
import { NOTICE_MOUNT, NOTICE_UNMOUNT, NOTICES_PORT, isMountAnswer } from '@/lib/notices/messages';

const MENU_ID = 'wirecatch-download';

/** "Download with Wirecatch" on links and media, created once per install or update. */
function registerContextMenu(hub: Hub): void {
  browser.runtime.onInstalled.addListener(() => {
    browser.contextMenus
      .removeAll()
      .then(() => {
        browser.contextMenus.create({
          id: MENU_ID,
          title: 'Download with Wirecatch',
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
      announce: (tabId, result) => {
        // The page's top frame shows it next to where the person is looking; pages without
        // the content script (browser pages, other extensions) get it in the popup instead.
        const message: AnnounceMessage = {
          type: MEDIA_ANNOUNCE,
          ok: result.ok,
          message: result.message,
        };
        browser.tabs.sendMessage(tabId, message, { frameId: 0 }).catch((error: unknown) => {
          log.debug('media', 'The page cannot show the answer; the popup shows it', error);
          hub.notify(result.ok ? 'warning' : 'error', result.message);
        });
      },
    },
  );
}

export default defineBackground(() => {
  installDiagnostics('background', self);
  const client = new NativeClient({ transport: browserTransport(), hello: helloFromExtension });

  const registry = new MediaRegistry();
  const media: MediaCenter = new MediaCenter({
    registry,
    getTab: tabInfo,
    send: (request): Promise<MediaSendResult> => hub.sendMedia(request),
    store: mediaStore(),
    ignoredSites: () => hub.knownSettings()?.ignored_sites ?? [],
    videoSites: () => hub.knownSettings()?.video_sites ?? [],
    mediaButtonOn: () => hub.mediaButtonOn(),
    privateWindowsOn: () => hub.privateWindowsOn(),
    liveTabs,
  });
  // Started before any listener can fire, so every response waits for the restored lists.
  const restored = media.start();
  const watcher = new MediaWatcher(registry, {
    isPrivateTab: async (tabId) => (await browser.tabs.get(tabId)).incognito,
    privateWindowsOn: () => hub.privateWindowsOn(),
    ready: restored,
  });

  const notices = new NoticeCenter({
    focusedTab: () =>
      focusedWebTab({
        lastFocusedWindow: () => browser.windows.getLastFocused(),
        activeTab: async (windowId) => (await browser.tabs.query({ active: true, windowId }))[0],
        privateWindowsOn: () => hub.privateWindowsOn(),
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
    shown: (id) => hub.noticeShown(id),
    pressed: (id, button) => hub.noticePressed(id, button),
    now: () => Date.now(),
  });

  const hub: Hub = new Hub({
    media,
    mediaButtonSetting: { publish: publishButtonSetting },
    client,
    cache: settingsCache(),
    downloads: browserDownloads(),
    cookieHeader: cookieHeaderFor,
    hasSiteAccess,
    userAgent: navigator.userAgent,
    inFlight: inFlightStore(),
    alert: alertStore(),
    badge: toolbarBadge(),
    notices,
  });

  // Every listener is registered synchronously so a sleeping background wakes up for it.
  const downloadEvents: DownloadEvents = browser.downloads;
  registerCapture(downloadEvents, hub);
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
