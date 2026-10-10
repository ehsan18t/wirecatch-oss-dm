import type {
  AddedDownload,
  Event,
  ExtensionSettings,
  ExtensionSettingsPatch,
  NewDownload,
  QueueSummary,
  Reply,
} from '@wirecatch/protocol';
import { candidateFromItem, type DownloadItemLike } from './capture/candidate';
import { handOver, type DownloadControl, type HandoverResult } from './capture/handover';
import { shouldCapture } from './capture/should-capture';
import { fileNameFromUrl } from './file-types';
import {
  asPopupRequest,
  type BackgroundMessage,
  type Channel,
  type Notice,
  type PopupCommand,
  type Snapshot,
} from './messages';
import { RequestError, type NativeClient } from './native/client';
import type { ConnectionStatus } from './native/status';
import { applyPatch, isConfirmed } from './protocol';

/** The display cache kept in browser storage (D5: the app is the only real store). */
export interface CachedSettings {
  settings: ExtensionSettings;
  queues: QueueSummary[];
  savedAt: number;
}

export interface SettingsCache {
  load(): Promise<CachedSettings | null>;
  save(value: CachedSettings): Promise<void>;
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
  cookieHeader(url: string, storeId: string | null): Promise<string | null>;
  userAgent: string | null;
  badge?: Badge;
  now?: () => number;
  /** How long a paused download waits for the app to connect before it resumes in the browser. */
  connectWaitMs?: number;
  /** How long to wait for the app to answer a call. */
  requestTimeoutMs?: number;
}

/** What the context menu passes on a click (`contextMenus.OnClickData`). */
export interface MenuClick {
  linkUrl?: string;
  srcUrl?: string;
  pageUrl?: string;
  frameUrl?: string;
}

/** Used to rule out blob, data and internal addresses before anything is known about the app's settings. */
const CAPTURE_EVERYTHING: ExtensionSettings = {
  capture_enabled: true,
  ask_before_download: false,
  show_media_button: true,
  ignored_sites: [],
  capture_extensions: [],
  min_capture_size: 0,
  default_queue: 1,
};

function describeError(error: unknown): string {
  if (error instanceof RequestError) {
    if (error.kind === 'timeout') return 'Wirecatch did not answer in time.';
    return error.message;
  }
  return error instanceof Error ? error.message : String(error);
}

/**
 * The background script's state: the connection, the app's settings and queues,
 * open popups, and the capture and context menu flows.
 */
export class Hub {
  readonly #deps: HubDeps;
  readonly #now: () => number;
  readonly #popups = new Set<Channel<BackgroundMessage, unknown>>();
  readonly #inFlight = new Set<number>();
  #live: ExtensionSettings | null = null;
  #queues: QueueSummary[] = [];
  #cache: CachedSettings | null = null;
  #notice: Notice | null = null;

  constructor(deps: HubDeps) {
    this.#deps = deps;
    this.#now = deps.now ?? Date.now;
  }

  get #client(): NativeClient {
    return this.#deps.client;
  }

  /** Loads the display cache, then connects to the app. */
  async start(): Promise<void> {
    this.#client.onStatus((status) => this.#onStatus(status));
    this.#client.onEvent((event) => this.#onEvent(event));
    this.#client.start();
    try {
      const cached = await this.#deps.cache.load();
      // Settings fetched from the app while loading win over the cache.
      if (this.#cache === null) this.#cache = cached;
    } catch {
      // No cache yet, or storage unavailable: nothing to show until the app answers.
    }
    this.#publish();
  }

  snapshot(): Snapshot {
    const status = this.#client.status;
    const live = status.state === 'connected' ? this.#live : null;
    return {
      status,
      settings: live ?? this.#cache?.settings ?? null,
      settingsFromCache: live === null && this.#cache !== null,
      cachedAt: this.#cache?.savedAt ?? null,
      queues: live !== null ? this.#queues : (this.#cache?.queues ?? []),
      notice: this.#notice,
    };
  }

  /** Connects an open popup: it gets a snapshot now and after every change. */
  addPopup(channel: Channel<BackgroundMessage, unknown>): void {
    this.#popups.add(channel);
    channel.onDisconnect.addListener(() => this.#popups.delete(channel));
    channel.onMessage.addListener((raw) => {
      const request = asPopupRequest(raw);
      if (request === null) return;
      void this.#runCommand(request.command).then(
        () => this.#post(channel, { type: 'result', id: request.id, ok: true }),
        (error: unknown) =>
          this.#post(channel, { type: 'result', id: request.id, ok: false, message: describeError(error) }),
      );
    });
    this.#post(channel, { type: 'snapshot', snapshot: this.snapshot() });
  }

  /**
   * Called for every new browser download. Decides quickly from the settings at
   * hand, then runs the handover (which checks the app's live settings again).
   * Resolves with null when the download was not even considered.
   */
  async onDownload(item: DownloadItemLike): Promise<HandoverResult | null> {
    if (this.#inFlight.has(item.id)) return null;
    const candidate = candidateFromItem(item);
    const known = this.#live ?? this.#cache?.settings ?? CAPTURE_EVERYTHING;
    if (!shouldCapture(candidate, known).capture) return null;

    this.#inFlight.add(item.id);
    try {
      const result = await handOver(
        { downloadId: item.id, candidate, cookieStoreId: item.cookieStoreId ?? null },
        {
          downloads: this.#deps.downloads,
          liveSettings: () => this.#liveSettings(),
          addDownload: (download) => this.#addDownload(download),
          cookieHeader: (url, storeId) => this.#deps.cookieHeader(url, storeId),
          userAgent: this.#deps.userAgent,
        },
      );
      this.#noticeFor(result, candidate.fileName ?? fileNameFromUrl(candidate.url) ?? 'A download');
      return result;
    } finally {
      this.#inFlight.delete(item.id);
    }
  }

  /**
   * "Download with Wirecatch" from the context menu. If the app does not take the
   * link, the browser downloads it instead, so the click is never lost.
   */
  async onMenuClick(click: MenuClick): Promise<'handed_over' | 'browser' | 'failed'> {
    const url = click.linkUrl ?? click.srcUrl;
    if (url === undefined || url === '') return 'failed';
    const isWeb = /^https?:/i.test(url);
    const name = (isWeb ? fileNameFromUrl(url) : null) ?? 'this item';

    if (!isWeb) {
      return this.#browserFallback(
        url,
        `Wirecatch can only take web links, so the browser is downloading ${name} instead.`,
      );
    }
    let reason: string;
    try {
      const added = await this.#addDownload({
        url,
        referrer: click.frameUrl ?? click.pageUrl ?? null,
        page_url: click.pageUrl ?? null,
        suggested_name: null,
        mime: null,
        size_hint: null,
        cookies: await this.#deps.cookieHeader(url, null),
        user_agent: this.#deps.userAgent,
        extra_headers: [],
        source: 'context_menu',
        queue: this.#live?.default_queue ?? null,
      });
      if (isConfirmed(added.action)) return 'handed_over';
      reason = 'Wirecatch gave an unexpected answer.';
    } catch (error) {
      reason = describeError(error);
    }
    return this.#browserFallback(
      url,
      `Wirecatch could not take ${name}, so the browser is downloading it instead. ${reason}`,
    );
  }

  async #browserFallback(url: string, message: string): Promise<'browser' | 'failed'> {
    try {
      await this.#deps.downloads.start(url);
      this.#setNotice({ tone: 'warning', message, at: this.#now() });
      return 'browser';
    } catch {
      this.#setNotice({
        tone: 'error',
        message: 'This item could not be downloaded. Try the browser\'s own "Save as" option instead.',
        at: this.#now(),
      });
      return 'failed';
    }
  }

  async #addDownload(download: NewDownload): Promise<AddedDownload> {
    const reply = await this.#client.request(
      { method: 'add_download', params: download },
      this.#deps.requestTimeoutMs ?? 10_000,
    );
    if (reply.type !== 'download_added') {
      throw new Error('Wirecatch gave an unexpected answer.');
    }
    return reply.data;
  }

  /** The app's settings, connecting first if needed; null when the app cannot be reached in time. */
  async #liveSettings(): Promise<ExtensionSettings | null> {
    if (this.#live !== null && this.#client.status.state === 'connected') return this.#live;
    try {
      const reply = await this.#client.request(
        { method: 'get_extension_settings' },
        this.#deps.connectWaitMs ?? 6_000,
      );
      if (reply.type !== 'extension_settings') return null;
      this.#adoptSettings(reply.data);
      return reply.data;
    } catch {
      return null;
    }
  }

  async #runCommand(command: PopupCommand): Promise<void> {
    switch (command.type) {
      case 'reconnect':
        this.#client.reconnectNow();
        return;
      case 'dismiss_notice':
        this.#setNotice(null);
        return;
      case 'pause_all':
        await this.#call({ method: 'pause_all' });
        return;
      case 'resume_all':
        await this.#call({ method: 'resume_all' });
        return;
      case 'update_settings':
        await this.#updateSettings(command.patch);
        return;
    }
  }

  async #updateSettings(patch: ExtensionSettingsPatch): Promise<void> {
    const reply = await this.#call({ method: 'update_extension_settings', params: patch });
    if (reply.type === 'extension_settings') {
      this.#adoptSettings(reply.data);
    } else if (this.#live !== null) {
      // The app only said "done"; its `extension_settings_changed` event confirms the values.
      this.#adoptSettings(applyPatch(this.#live, patch));
    }
  }

  async #call(call: Parameters<NativeClient['request']>[0]): Promise<Reply> {
    if (this.#client.status.state !== 'connected') {
      throw new Error('Wirecatch is not connected, so this cannot be done right now.');
    }
    return this.#client.request(call, this.#deps.requestTimeoutMs ?? 10_000);
  }

  #onStatus(status: ConnectionStatus): void {
    if (status.state === 'connected') {
      void this.#refresh();
    } else {
      this.#live = null;
    }
    this.#publish();
  }

  async #refresh(): Promise<void> {
    try {
      const timeout = this.#deps.requestTimeoutMs ?? 10_000;
      const [settings, queues] = await Promise.all([
        this.#client.request({ method: 'get_extension_settings' }, timeout),
        this.#client.request({ method: 'list_queues' }, timeout),
      ]);
      if (queues.type === 'queues') this.#queues = queues.data;
      if (settings.type === 'extension_settings') this.#adoptSettings(settings.data);
      else this.#publish();
    } catch {
      // The status listener already reports a broken connection; a slow answer is retried on the next connection.
    }
  }

  #onEvent(event: Event): void {
    switch (event.type) {
      case 'extension_settings_changed':
        this.#adoptSettings(event.data);
        return;
      case 'queues_changed':
        this.#queues = event.data;
        this.#saveCache();
        this.#publish();
        return;
    }
  }

  #adoptSettings(settings: ExtensionSettings): void {
    this.#live = settings;
    this.#saveCache();
    this.#publish();
  }

  #saveCache(): void {
    if (this.#live === null) return;
    const value: CachedSettings = { settings: this.#live, queues: this.#queues, savedAt: this.#now() };
    this.#cache = value;
    this.#deps.cache.save(value).catch(() => {
      // The cache is only for display; a failed write changes nothing else.
    });
  }

  #noticeFor(result: HandoverResult, name: string): void {
    if (result.kind === 'handed_over') {
      if (!result.browserCopyRemoved) {
        this.#setNotice({
          tone: 'warning',
          message: `${name} went to Wirecatch, but its paused copy is still in the browser's download list. You can remove it there.`,
          at: this.#now(),
        });
      }
      return;
    }
    if (!result.resumed) {
      this.#setNotice({
        tone: 'error',
        message: `${name} is paused in the browser. Open the browser's downloads page and click Resume.`,
        at: this.#now(),
      });
      return;
    }
    if (result.reason === 'app_unreachable' || result.reason === 'app_refused') {
      const why =
        result.reason === 'app_unreachable'
          ? 'Wirecatch could not be reached'
          : (result.detail ?? 'Wirecatch did not accept it');
      this.#setNotice({
        tone: 'warning',
        message: `${name} stayed in the browser: ${why.replace(/\.$/, '')}.`,
        at: this.#now(),
      });
    }
  }

  #setNotice(notice: Notice | null): void {
    this.#notice = notice;
    this.#publish();
  }

  #publish(): void {
    const snapshot = this.snapshot();
    for (const popup of this.#popups) this.#post(popup, { type: 'snapshot', snapshot });
    this.#updateBadge(snapshot);
  }

  #post(channel: Channel<BackgroundMessage, unknown>, message: BackgroundMessage): void {
    try {
      channel.postMessage(message);
    } catch {
      this.#popups.delete(channel);
    }
  }

  #updateBadge(snapshot: Snapshot): void {
    const badge = this.#deps.badge;
    if (badge === undefined) return;
    if (snapshot.status.state === 'unavailable') {
      badge.set('!', 'Wirecatch is not connected. Click for details.');
    } else if (snapshot.notice !== null) {
      badge.set('!', 'Wirecatch has a message for you. Click to read it.');
    } else {
      badge.set('', 'Wirecatch');
    }
  }
}
