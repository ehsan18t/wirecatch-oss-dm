import type {
  AddedDownload,
  Call,
  Event,
  ExtensionSettings,
  ExtensionSettingsPatch,
  NewDownload,
  Outcome,
  QueueSummary,
  Reply,
  WithdrawOutcome,
} from '@wirecatch/protocol';
import { candidateFromItem, type DownloadItemLike } from './capture/candidate';
import {
  handOver,
  resumeBrowserCopy,
  submit,
  type DownloadControl,
  type HandoverResult,
  type JudgingSettings,
} from './capture/handover';
import { shouldCapture } from './capture/should-capture';
import type { CookieContext } from './cookies';
import { fileNameFromUrl, isProgramFile } from './file-types';
import { refererFor } from './referrer';
import type { MediaRequest, MediaSendResult, MediaView } from './media/center';
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

/**
 * Ids of browser downloads paused for a handover, kept in `storage.session` so a
 * background script that is restarted mid-handover can resume them.
 */
export interface InFlightStore {
  load(): Promise<number[]>;
  save(ids: number[]): Promise<void>;
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
  newHandoverId?: () => string;
  badge?: Badge;
  now?: () => number;
  /** How long a paused download waits for the app to connect before it resumes in the browser. */
  connectWaitMs?: number;
  /** How long to wait for the app to answer a call. */
  requestTimeoutMs?: number;
  /** How long to wait for the answer to `withdraw_download`. */
  withdrawTimeoutMs?: number;
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
  incognito?: boolean;
  /** Firefox container or private cookie store of the tab. */
  cookieStoreId?: string;
}

export type MenuResult = 'handed_over' | 'browser' | 'failed' | 'private_window';

/** Used to rule out blob, data and internal addresses before anything is known about the settings. */
const CAPTURE_EVERYTHING: ExtensionSettings = {
  capture_enabled: true,
  ask_before_download: false,
  show_media_button: true,
  ignored_sites: [],
  capture_extensions: [],
  min_capture_size: 0,
  default_queue: 1,
};

/** How many handover ids are remembered for spotting late answers. */
const RECENT_LIMIT = 50;

const NO_SITE_ACCESS =
  'Wirecatch needs access to websites to take downloads with their cookies. Open this popup and click Allow access.';

/** For media there is no browser download to fall back on, so the person has to act. */
export const MEDIA_UNREACHABLE = 'Wirecatch could not be reached. Open Wirecatch, then try again.';
const MEDIA_NO_SITE_ACCESS =
  'Wirecatch needs access to websites to send media with its cookies. Open the Wirecatch popup and click Allow access.';

function describeError(error: unknown): string {
  if (error instanceof RequestError) {
    if (error.kind === 'timeout') return 'Wirecatch did not answer in time.';
    return error.message;
  }
  return error instanceof Error ? error.message : String(error);
}

function remember<K, V>(map: Map<K, V>, key: K, value: V): void {
  map.set(key, value);
  if (map.size > RECENT_LIMIT) {
    const oldest = map.keys().next();
    if (oldest.done !== true) map.delete(oldest.value);
  }
}

/**
 * The background script's state: the connection, the app's settings and queues,
 * open popups, and the capture and context menu flows.
 */
export class Hub {
  readonly #deps: HubDeps;
  readonly #now: () => number;
  readonly #popups = new Set<Channel<BackgroundMessage, unknown>>();
  /** Browser downloads being handed over right now (persisted through `deps.inFlight`). */
  readonly #inFlight = new Set<number>();
  /** Handovers kept in the browser although the app may have them (the withdraw failed). */
  readonly #uncertain = new Map<string, string>();
  /** Handovers the app confirmed only after this side stopped waiting. */
  readonly #lateConfirmed = new Map<string, string>();
  readonly #ready: Promise<void>;
  #markReady: () => void = () => undefined;
  #live: ExtensionSettings | null = null;
  #queues: QueueSummary[] = [];
  #cache: CachedSettings | null = null;
  #notice: Notice | null = null;

  constructor(deps: HubDeps) {
    this.#deps = deps;
    this.#now = deps.now ?? Date.now;
    this.#ready = new Promise((resolve) => {
      this.#markReady = resolve;
    });
  }

  get #client(): NativeClient {
    return this.#deps.client;
  }

  /** Resolves once the display cache is loaded, so downloads are never judged without it. */
  get ready(): Promise<void> {
    return this.#ready;
  }

  /** Connects to the app, loads the display cache and resumes downloads a previous run left paused. */
  async start(): Promise<void> {
    this.#client.onStatus((status) => this.#onStatus(status));
    this.#client.onEvent((event) => this.#onEvent(event));
    this.#client.onLateAnswer((call, outcome) => this.#onLateAnswer(call, outcome));
    this.#client.start();
    // Both are read before `ready`, so no new download can overwrite the list of paused ones first.
    const [cached, leftPaused] = await Promise.all([
      this.#deps.cache.load().catch(() => null),
      this.#deps.inFlight?.load().catch(() => []) ?? Promise.resolve([]),
    ]);
    // Settings fetched from the app while loading win over the cache.
    if (this.#cache === null) this.#cache = cached;
    this.#markReady();
    this.#publish();
    await this.#recoverInFlight(leftPaused);
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

  /** Connects an open popup: it gets a snapshot once the cache is loaded, and after every change. */
  addPopup(channel: Channel<BackgroundMessage, unknown>): void {
    this.#popups.add(channel);
    let stopWatching: (() => void) | null = null;
    channel.onDisconnect.addListener(() => {
      this.#popups.delete(channel);
      stopWatching?.();
    });
    channel.onMessage.addListener((raw) => {
      const request = asPopupRequest(raw);
      if (request === null) return;
      const { command } = request;
      if (command.type === 'watch_media') {
        // One list per popup: the active tab's.
        stopWatching?.();
        stopWatching =
          this.#deps.media?.watch(command.tabId, (view) => this.#post(channel, { type: 'media', view })) ??
          null;
        this.#post(channel, { type: 'result', id: request.id, ok: true });
        return;
      }
      void this.#runCommand(command).then(
        (message) =>
          this.#post(
            channel,
            message === undefined
              ? { type: 'result', id: request.id, ok: true }
              : { type: 'result', id: request.id, ok: true, message },
          ),
        (error: unknown) =>
          this.#post(channel, { type: 'result', id: request.id, ok: false, message: describeError(error) }),
      );
    });
    void this.#ready.then(() => this.#post(channel, { type: 'snapshot', snapshot: this.snapshot() }));
  }

  /**
   * Called for every new browser download. Decides from the settings at hand
   * (never before the cache is loaded), then runs the handover, which checks the
   * settings again after pausing. Resolves with null when the download was not
   * touched at all.
   */
  async onDownload(item: DownloadItemLike): Promise<HandoverResult | null> {
    if (this.#inFlight.has(item.id)) return null;
    this.#inFlight.add(item.id);
    let persisted = false;
    try {
      await this.#ready;
      const candidate = candidateFromItem(item);
      const known = this.#live ?? this.#cache?.settings ?? CAPTURE_EVERYTHING;
      if (!shouldCapture(candidate, known).capture) return null;
      const name = candidate.fileName ?? fileNameFromUrl(candidate.url) ?? 'A download';

      if (!(await this.#siteAccess(candidate.url))) {
        this.#setNotice({
          tone: 'warning',
          message: `${name} stayed in the browser. ${NO_SITE_ACCESS}`,
          at: this.#now(),
        });
        return null;
      }

      // Recorded before pausing, so a restarted background can resume it.
      await this.#persistInFlight();
      persisted = true;
      const result = await handOver(
        { downloadId: item.id, candidate, cookieStoreId: item.cookieStoreId ?? null },
        {
          downloads: this.#deps.downloads,
          liveSettings: () => this.#judgingSettings(),
          addDownload: (download) => this.#addDownload(download),
          withdraw: (handoverId) => this.#withdraw(handoverId),
          cookieHeader: (url, storeId, context) => this.#deps.cookieHeader(url, storeId, context),
          userAgent: this.#deps.userAgent,
          newHandoverId: () => this.#newHandoverId(),
        },
      );
      this.#noticeFor(result, name);
      return result;
    } finally {
      this.#inFlight.delete(item.id);
      if (persisted) await this.#persistInFlight();
    }
  }

  /**
   * "Download with Wirecatch" from the context menu. If the app does not take the
   * link, the browser downloads it instead, so the click is never lost.
   */
  async onMenuClick(click: MenuClick, tab: MenuTab | undefined): Promise<MenuResult> {
    const url = click.linkUrl ?? click.srcUrl;
    if (url === undefined || url === '') return 'failed';
    if (tab?.incognito === true) {
      // Like browser downloads from private windows, these stay out of Wirecatch's list.
      this.#setNotice({
        tone: 'warning',
        message:
          'Wirecatch does not take downloads from private windows. Use the browser\'s own "Save as" option there.',
        at: this.#now(),
      });
      return 'private_window';
    }
    const isWeb = /^https?:/i.test(url);
    const name = (isWeb ? fileNameFromUrl(url) : null) ?? 'this item';

    if (!isWeb) {
      return this.#browserFallback(
        url,
        `Wirecatch can only take web links, so the browser is downloading ${name} instead.`,
      );
    }
    if (isProgramFile(fileNameFromUrl(url), null)) {
      return this.#browserFallback(
        url,
        `${name} is a program, so the browser is downloading it instead and can check it for harmful software.`,
      );
    }
    if (!(await this.#siteAccess(url))) {
      return this.#browserFallback(url, `The browser is downloading ${name} instead. ${NO_SITE_ACCESS}`);
    }
    await this.#ready;
    const handoverId = this.#newHandoverId();
    const result = await submit(
      {
        handover_id: handoverId,
        url,
        // What the browser itself would send (strict-origin-when-cross-origin), never more.
        referrer: refererFor(url, click.frameUrl ?? click.pageUrl),
        page_url: click.pageUrl ?? null,
        suggested_name: null,
        mime: null,
        size_hint: null,
        // Like following the link: a top-level navigation started by the frame it is in.
        cookies: await this.#deps.cookieHeader(url, tab?.cookieStoreId ?? null, {
          initiator: click.frameUrl ?? click.pageUrl ?? null,
          navigation: true,
        }),
        user_agent: this.#deps.userAgent,
        extra_headers: [],
        source: 'context_menu',
        queue: this.#live?.default_queue ?? null,
      },
      {
        addDownload: (download) => this.#addDownload(download),
        withdraw: (id) => this.#withdraw(id),
      },
    );
    if (result.taken) return 'handed_over';
    if (result.uncertain) this.#markUncertain(handoverId, name);
    const reason = result.detail === null ? '' : ` ${result.detail}`;
    return this.#browserFallback(
      url,
      `Wirecatch could not take ${name}, so the browser is downloading it instead.${reason}`,
    );
  }

  /**
   * Hands media to the app (from the media button or the popup). Uses the same
   * handover as downloads: a fresh handover id, and a withdraw when the answer is
   * lost. There is no browser download to keep, so when the app cannot be reached
   * the answer says to open Wirecatch instead of failing quietly.
   */
  async sendMedia(request: MediaRequest): Promise<MediaSendResult> {
    const { target } = request;
    if (!(await this.#siteAccess(target.url))) return { ok: false, message: MEDIA_NO_SITE_ACCESS };
    await this.#ready;
    if (this.#client.status.state !== 'connected' || this.#live === null) {
      // Connect (and learn the default queue) first, so "could not be reached" is certain:
      // nothing has been sent yet when this fails.
      try {
        const reply = await this.#client.request(
          { method: 'get_extension_settings' },
          this.#deps.connectWaitMs ?? 6_000,
        );
        if (reply.type === 'extension_settings') this.#adoptSettings(reply.data);
      } catch {
        return { ok: false, message: MEDIA_UNREACHABLE };
      }
    }
    const result = await submit(
      {
        handover_id: this.#newHandoverId(),
        url: target.url,
        referrer: refererFor(target.url, request.referrer),
        page_url: request.pageUrl,
        suggested_name: null,
        mime: target.mime,
        size_hint: target.size,
        // Media is loaded by the page itself (a top-level navigation only for the page address).
        cookies: await this.#deps.cookieHeader(target.url, request.cookieStoreId, {
          initiator: request.referrer,
          topLevel: request.pageUrl,
          navigation: target.kind === 'page',
        }),
        user_agent: this.#deps.userAgent,
        extra_headers: [],
        source: 'media_button',
        queue: this.#live?.default_queue ?? null,
        media: { kind: target.kind, title: request.title },
      },
      {
        addDownload: (download) => this.#addDownload(download),
        withdraw: (id) => this.#withdraw(id),
      },
    );
    if (result.taken) {
      return {
        ok: true,
        message:
          result.action === 'awaiting_confirmation'
            ? 'Sent to Wirecatch. Confirm it there to start.'
            : 'Sent to Wirecatch.',
      };
    }
    if (result.reason === 'refused') {
      const why = result.detail === null ? '' : ` ${result.detail.replace(/\.?$/, '.')}`;
      return { ok: false, message: `Wirecatch did not take it.${why}` };
    }
    return {
      ok: false,
      message: result.uncertain
        ? 'Wirecatch did not answer in time. Open Wirecatch and check its list before trying again.'
        : 'Wirecatch did not answer in time, so nothing was added. Open Wirecatch, then try again.',
    };
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

  async #siteAccess(url: string): Promise<boolean> {
    try {
      return await this.#deps.hasSiteAccess(url);
    } catch {
      return false;
    }
  }

  #newHandoverId(): string {
    return this.#deps.newHandoverId?.() ?? crypto.randomUUID();
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

  async #withdraw(handoverId: string): Promise<WithdrawOutcome> {
    const reply = await this.#client.request(
      { method: 'withdraw_download', params: { handover_id: handoverId } },
      this.#deps.withdrawTimeoutMs ?? 3_000,
    );
    if (reply.type !== 'withdrawn') throw new Error('Wirecatch gave an unexpected answer.');
    return reply.data;
  }

  /**
   * The settings to judge a paused download by: the app's, connecting first if
   * needed; when the app cannot be reached in time, the cached copy (marked as
   * such, so a cached "capture off" still keeps the download in the browser).
   */
  async #judgingSettings(): Promise<JudgingSettings | null> {
    if (this.#live !== null && this.#client.status.state === 'connected') {
      return { settings: this.#live, fromApp: true };
    }
    try {
      const reply = await this.#client.request(
        { method: 'get_extension_settings' },
        this.#deps.connectWaitMs ?? 6_000,
      );
      if (reply.type === 'extension_settings') {
        this.#adoptSettings(reply.data);
        return { settings: reply.data, fromApp: true };
      }
    } catch {
      // Unreachable: fall back to the cache below.
    }
    return this.#cache === null ? null : { settings: this.#cache.settings, fromApp: false };
  }

  async #persistInFlight(): Promise<void> {
    try {
      await this.#deps.inFlight?.save([...this.#inFlight]);
    } catch {
      // Session storage unavailable: recovery after a restart is best effort.
    }
  }

  /** Resumes downloads a previous run of the background script paused and never finished handing over. */
  async #recoverInFlight(ids: readonly number[]): Promise<void> {
    if (ids.length === 0) return;
    for (const id of ids) {
      if (this.#inFlight.has(id)) continue;
      const state = await this.#deps.downloads.state(id).catch(() => null);
      if (state?.state === 'in_progress' && state.paused) {
        await resumeBrowserCopy(this.#deps.downloads, id);
      }
    }
    await this.#persistInFlight();
  }

  /** Runs a popup command; resolves with a sentence to show when there is one. */
  async #runCommand(command: PopupCommand): Promise<string | undefined> {
    switch (command.type) {
      case 'watch_media':
        return undefined;
      case 'download_media': {
        const media = this.#deps.media;
        if (media === undefined) throw new Error('Media cannot be sent from here.');
        const result = await media.download(command.tabId, command.key);
        if (!result.ok) throw new Error(result.message);
        return result.message;
      }
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

  async #call(call: Call): Promise<Reply> {
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

  /** The app confirmed a handover after this side gave up waiting: warn if the browser kept its copy too. */
  #onLateAnswer(call: Call, outcome: Outcome): void {
    if (call.method !== 'add_download' || outcome.status !== 'ok') return;
    const reply = outcome.value;
    if (reply.type !== 'download_added' || !isConfirmed(reply.data.action)) return;
    const id = call.params.handover_id;
    const name = call.params.suggested_name ?? fileNameFromUrl(call.params.url) ?? 'A download';
    if (this.#uncertain.has(id)) {
      this.#uncertain.delete(id);
      this.#warnDuplicate(name);
    } else {
      remember(this.#lateConfirmed, id, name);
    }
  }

  /** Remembers a handover the app may still have; returns true if its late answer already came (and warns). */
  #markUncertain(handoverId: string, name: string): boolean {
    if (this.#lateConfirmed.has(handoverId)) {
      this.#lateConfirmed.delete(handoverId);
      this.#warnDuplicate(name);
      return true;
    }
    remember(this.#uncertain, handoverId, name);
    return false;
  }

  #warnDuplicate(name: string): void {
    this.#setNotice({
      tone: 'warning',
      message: `${name} may be downloading twice: Wirecatch took it after all, and the browser kept its copy too. You can cancel one of them.`,
      at: this.#now(),
    });
  }

  #adoptSettings(settings: ExtensionSettings): void {
    this.#live = settings;
    this.#saveCache();
    this.#publishButtonSetting(settings.show_media_button);
    this.#publish();
  }

  #buttonSetting: boolean | null = null;

  #publishButtonSetting(on: boolean): void {
    if (this.#buttonSetting === on) return;
    this.#buttonSetting = on;
    this.#deps.mediaButtonSetting?.save(on).catch(() => {
      // Not saved: the buttons keep the previous setting until the next change.
    });
  }

  /** The app's settings, or the cached copy when the app is not connected; null before either is known. */
  knownSettings(): ExtensionSettings | null {
    return this.#live ?? this.#cache?.settings ?? null;
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
    // The person cancelled it in the browser meanwhile, or it finished: nothing to say.
    if (result.browserCopy === 'gone') return;
    if (result.browserCopy === 'stuck_paused') {
      this.#setNotice({
        tone: 'error',
        message: `${name} is paused in the browser. Open the browser's downloads page and click Resume.`,
        at: this.#now(),
      });
      return;
    }
    if (result.uncertain && result.handoverId !== null && this.#markUncertain(result.handoverId, name)) {
      return;
    }
    if (
      result.reason === 'app_unreachable' ||
      result.reason === 'app_refused' ||
      result.reason === 'no_answer'
    ) {
      const why =
        result.reason === 'app_unreachable'
          ? 'Wirecatch could not be reached'
          : result.reason === 'no_answer'
            ? 'Wirecatch did not answer in time'
            : (result.detail ?? 'Wirecatch did not accept it');
      this.#setNotice({
        tone: 'warning',
        message: `${name} stayed in the browser: ${why.replace(/\.$/, '')}.`,
        at: this.#now(),
      });
    }
  }

  /** Shows a message in the popup (and the badge), for answers that have nowhere else to go. */
  notify(tone: Notice['tone'], message: string): void {
    this.#setNotice({ tone, message, at: this.#now() });
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
