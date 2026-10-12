/** What the hub is given: the connection, browser APIs and stores, and the shapes it hands back. */
import type { ExtensionSettings, QueueSummary } from '@wirecatch/protocol';
import type { DownloadControl } from '../capture/handover';
import type { CookieContext } from '../cookies';
import type { MediaSendResult, MediaView } from '../media/center';
import type { Notice } from '../messages';
import type { NativeClient } from '../native/client';

/** The display cache kept in browser storage (D5: the app is the only real store). */
export interface CachedSettings {
  settings: ExtensionSettings;
  queues: QueueSummary[];
  savedAt: number;
  /** The app was closed when last seen, so waking does not start a bridge only to find that out again. */
  appClosed: boolean;
}

export interface SettingsCache {
  load(): Promise<CachedSettings | null>;
  save(value: CachedSettings): Promise<void>;
}

/** A browser download paused for a handover, and the handover id it is (or will be) offered under. */
export interface InFlightEntry {
  id: number;
  handoverId: string;
}

/**
 * The browser downloads paused for a handover, kept in `storage.session` so a background
 * script that is restarted mid-handover can withdraw each one and act on the answer. The
 * area is cleared when the extension is updated, so it never holds another version's shape.
 */
export interface InFlightStore {
  load(): Promise<InFlightEntry[]>;
  save(entries: InFlightEntry[]): Promise<void>;
}

/**
 * The notice the popup and the badge show, kept in `storage.session` next to the
 * in-flight downloads, so one about a download left paused outlives a background
 * script the browser stopped.
 */
export interface NoticeStore {
  load(): Promise<Notice[]>;
  save(notices: Notice[]): Promise<void>;
}

export interface BrowserDownloads extends DownloadControl {
  /** Starts a download in the browser itself (`downloads.download`), used when the app cannot take a context menu link. */
  start(url: string): Promise<void>;
}

export interface Badge {
  set(text: string, title: string): void;
}

export interface HubDeps {
  client: NativeClient;
  cache: SettingsCache;
  downloads: BrowserDownloads;
  /** The `Cookie` header the browser would send to a URL in this context, or null. Never rejects. */
  cookieHeader(url: string, storeId: string | null, context: CookieContext): Promise<string | null>;
  /** True when the extension may read cookies for this address (the person can revoke site access in Firefox). */
  hasSiteAccess(url: string): Promise<boolean>;
  userAgent: string | null;
  inFlight?: InFlightStore;
  notice?: NoticeStore;
  newHandoverId?: () => string;
  badge?: Badge;
  now?: () => number;
  /** How long to wait for the app; each value missing here is taken from `DEFAULT_TIMING`. */
  timing?: Partial<HubTiming>;
  /** Publishes the "Show download button on videos" setting where content scripts read it (`MEDIA_BUTTON_KEY`). */
  mediaButtonSetting?: { save(on: boolean): Promise<void> };
  /** The media lists the popup shows (`MediaCenter`); without it the popup's media commands fail. */
  media?: MediaCommands;
}

/** What the popup's media commands need from the background's media side. */
export interface MediaCommands {
  watch(tabId: number, push: (view: MediaView) => void): () => void;
  download(tabId: number, key: string): Promise<MediaSendResult>;
}

/** What the context menu passes on a click (`contextMenus.OnClickData`), plus the tab it happened in. */
export interface MenuClick {
  linkUrl?: string;
  srcUrl?: string;
  pageUrl?: string;
  frameUrl?: string;
}

export interface MenuTab {
  incognito: boolean;
  /** Firefox container or private cookie store of the tab; null in Chromium. */
  cookieStoreId: string | null;
}

export type MenuResult = 'handed_over' | 'browser' | 'failed' | 'private_window';

/** How long the hub waits for the app. */
export interface HubTiming {
  /** How long a paused download, or media about to be sent, waits for the app to connect. */
  connectWaitMs: number;
  /** How long a call waits for the app's answer. */
  requestTimeoutMs: number;
  /** How long to wait for the answer to `withdraw_download`. */
  withdrawTimeoutMs: number;
}
