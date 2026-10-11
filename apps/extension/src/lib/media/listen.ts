/**
 * The browser events the media side listens to, registered in one place so the
 * registration itself (filters, sender checks, browser differences) can be tested
 * without a browser. The background script passes the real `browser` APIs.
 */
import { SITE_ORIGINS } from '../site-access';
import type { ContentSender, MediaCenter, MediaSendResult } from './center';
import type { ObservedResponse } from './detect';
import { MEDIA_COMMAND } from './messages';
import type { MediaWatcher } from './watch';

export interface RequestFilter {
  urls: string[];
  types: string[];
}

interface Event<L> {
  addListener(listener: L): void;
}

/** The parts of the extension APIs this module uses. */
export interface MediaEventApi {
  onResponseStarted: {
    addListener(listener: (details: ObservedResponse) => void, filter: RequestFilter, extra: string[]): void;
  };
  onTabRemoved: Event<(tabId: number) => void>;
  onTabUpdated: {
    addListener(
      listener: (tabId: number, change: { url?: string }) => void,
      filter?: { properties: string[] },
    ): void;
  };
  onMessage: Event<
    (
      message: unknown,
      sender: ContentSender & { id?: string },
      sendResponse: (response: unknown) => void,
    ) => boolean | undefined
  >;
  /** Keyboard shortcuts (`commands.onCommand`); the tab is where the shortcut was pressed. */
  onCommand: Event<(command: string, tab?: { id?: number }) => void>;
  /** This extension's id (`runtime.id`). */
  extensionId: string;
  /** Firefox lets `tabs.onUpdated` be limited to address changes; Chromium refuses the argument. */
  firefox: boolean;
}

/**
 * Responses that load whole media files or pages: every web address, but only the
 * request types a media element, a frame or the tab itself makes.
 */
export const FILE_REQUESTS: RequestFilter = {
  urls: SITE_ORIGINS,
  types: ['main_frame', 'sub_frame', 'media', 'object'],
};

/**
 * Responses fetched by page scripts, which is how players load HLS and DASH
 * manifests. Script requests are by far the most frequent kind (every API call of
 * every page), so they are observed only for addresses that look like a manifest,
 * and the background is not woken for the rest. The trade-off: a manifest fetched by
 * a script from an address without `.m3u8` or `.mpd` (recognisable only by its
 * content type) is not seen. Manifests a media element loads itself, and every other
 * resource type, are still recognised by content type alone.
 */
export const MANIFEST_REQUESTS: RequestFilter = {
  urls: ['*://*/*.m3u8*', '*://*/*.M3U8*', '*://*/*.mpd*', '*://*/*.MPD*'],
  types: ['xmlhttprequest', 'other'],
};

export function registerMediaListeners(
  api: MediaEventApi,
  handlers: {
    watcher: MediaWatcher;
    media: MediaCenter;
    /** Shows the shortcut's answer on the page (or elsewhere when the page cannot show it). */
    announce: (tabId: number, result: MediaSendResult) => void;
  },
): void {
  const { watcher, media, announce } = handlers;
  api.onCommand.addListener((command, tab) => {
    const tabId = tab?.id;
    if (command !== MEDIA_COMMAND || tabId === undefined || tabId < 0) return;
    media.downloadBest(tabId).then(
      (result) => announce(tabId, result),
      () => undefined,
    );
  });
  const onResponse = (details: ObservedResponse): void => {
    watcher.onResponse(details).catch(() => undefined);
  };
  // Observe only: no `blocking` option, so requests are never held up or changed.
  api.onResponseStarted.addListener(onResponse, FILE_REQUESTS, ['responseHeaders']);
  api.onResponseStarted.addListener(onResponse, MANIFEST_REQUESTS, ['responseHeaders']);

  api.onTabRemoved.addListener((tabId) => watcher.onTabRemoved(tabId));
  // Address changes without a new document (history API, back-forward cache).
  const onUpdated = (tabId: number, change: { url?: string }): void => {
    if (change.url !== undefined) watcher.onUrlChange(tabId, change.url).catch(() => undefined);
  };
  if (api.firefox) api.onTabUpdated.addListener(onUpdated, { properties: ['url'] });
  else api.onTabUpdated.addListener(onUpdated);

  // The media button's questions, from this extension's content scripts only.
  // Answered through `sendResponse`, which both browsers support.
  api.onMessage.addListener((message, sender, sendResponse) => {
    if (sender.id !== api.extensionId || sender.tab === undefined) return undefined;
    const answer = media.handleContentMessage(message, sender);
    if (answer === null) return undefined;
    answer.then(sendResponse, () => sendResponse(null));
    return true;
  });
}
