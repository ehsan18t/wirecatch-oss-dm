import type { Call, Event, ExtensionSettings, Outcome } from '@wirecatch/protocol';
import { candidateFromItem, type DownloadItemLike } from '../capture/candidate';
import { handOver, type HandoverResult, type JudgingSettings } from '../capture/handover';
import { shouldCapture } from '../capture/should-capture';
import { fileNameFromUrl } from '../file-types';
import type { MediaRequest, MediaSendResult } from '../media/center';
import type { Notice, PopupCommand, Snapshot } from '../messages';
import type { NativeClient } from '../native/client';
import type { ConnectionStatus } from '../native/status';
import { refererFor } from '../referrer';
import { AppCalls } from './app';
import type { HubDeps, HubTiming, InFlightEntry, MenuClick, MenuResult, MenuTab } from './deps';
import { planMenuClick } from './menu';
import { NoticeBoard } from './notice-board';
import {
  BROWSER_DOWNLOAD_FAILED,
  MEDIA_NO_SITE_ACCESS,
  MEDIA_UNREACHABLE,
  MENU_PRIVATE_WINDOW,
  NO_SITE_ACCESS,
  mediaAnswer,
  menuNotTaken,
} from './notices';
import { Popups, type PopupChannel } from './popups';
import { recoverHandover } from './recovery';
import { KnownSettings } from './settings';

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

/**
 * The waits used unless `HubDeps.timing` says otherwise. A paused download waits less
 * than a call does, so the browser resumes it soon when the app cannot be reached; a
 * withdraw only runs after a call already took too long, so it waits the least.
 */
const DEFAULT_TIMING: HubTiming = {
  connectWaitMs: 6_000,
  requestTimeoutMs: 10_000,
  withdrawTimeoutMs: 3_000,
};

/**
 * The waits to use: each one given, or its `DEFAULT_TIMING` value. Field by field, so
 * one given as `undefined` still gets its default (a spread would copy the undefined).
 */
export function hubTiming(given: Partial<HubTiming> = {}): HubTiming {
  return {
    connectWaitMs: given.connectWaitMs ?? DEFAULT_TIMING.connectWaitMs,
    requestTimeoutMs: given.requestTimeoutMs ?? DEFAULT_TIMING.requestTimeoutMs,
    withdrawTimeoutMs: given.withdrawTimeoutMs ?? DEFAULT_TIMING.withdrawTimeoutMs,
  };
}

/**
 * The background script's state: the connection, the app's settings and queues,
 * open popups, and the capture and context menu flows.
 */
export class Hub {
  readonly #deps: HubDeps;
  readonly #now: () => number;
  readonly #settings: KnownSettings;
  readonly #app: AppCalls;
  readonly #popups: Popups;
  /**
   * Browser downloads being handed over or recovered right now, by id, with the handover id
   * once one is chosen; those with one are persisted through `deps.inFlight`.
   */
  readonly #inFlight = new Map<number, string | null>();
  readonly #ready: Promise<void>;
  #markReady: () => void = () => undefined;
  readonly #notices: NoticeBoard;

  constructor(deps: HubDeps) {
    this.#deps = deps;
    this.#now = deps.now ?? Date.now;
    const timing = hubTiming(deps.timing);
    this.#settings = new KnownSettings(deps, this.#now, () => this.#publish());
    this.#app = new AppCalls(deps.client, timing, this.#settings, deps.userAgent);
    this.#notices = new NoticeBoard({
      now: this.#now,
      store: deps.notice,
      changed: () => this.#publish(),
    });
    this.#popups = new Popups({ media: deps.media, run: (command) => this.#runCommand(command) });
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

  /**
   * Loads the display cache, connects to the app if it was running when last seen, and resumes downloads a previous run left paused.
   *
   * The background wakes for every web response and tab change. While the app is closed, connecting each time would start a bridge process only to learn that again, so it waits until something needs the app: a request (which starts it) or the popup (which shows whether it runs).
   */
  async start(): Promise<void> {
    this.#client.onStatus((status) => this.#onStatus(status));
    this.#client.onEvent((event) => this.#onEvent(event));
    this.#client.onLateAnswer((call, outcome) => this.#onLateAnswer(call, outcome));
    // All are read before `ready`, so no new download can overwrite the list of paused ones first.
    const [cached, leftPaused, notice] = await Promise.all([
      this.#deps.cache.load().catch(() => null),
      this.#deps.inFlight?.load().catch(() => []) ?? Promise.resolve([]),
      this.#deps.notice?.load().catch(() => null) ?? Promise.resolve(null),
    ]);
    this.#settings.loaded(cached);
    // A notice from before the browser stopped the background.
    this.#notices.restore(notice);
    if (cached?.appClosed !== true) this.#client.start();
    this.#markReady();
    this.#publish();
    await this.#recoverInFlight(leftPaused);
  }

  snapshot(): Snapshot {
    const status = this.#client.status;
    return {
      status,
      ...this.#settings.view(status.state === 'connected'),
      notice: this.#notices.current,
    };
  }

  /** Connects an open popup: it gets a snapshot once the cache is loaded, and after every change. */
  addPopup(channel: PopupChannel): void {
    this.#popups.add(channel);
    // The popup shows whether the app runs; this finds out if waking did not.
    this.#client.start();
    void this.#ready.then(() => this.#popups.publish(this.snapshot(), channel));
  }

  /**
   * Called for every new browser download. Decides from the settings at hand
   * (never before the cache is loaded), then runs the handover, which checks the
   * settings again after pausing. Resolves with null when the download was not
   * touched at all.
   */
  async onDownload(item: DownloadItemLike): Promise<HandoverResult | null> {
    if (this.#inFlight.has(item.id)) return null;
    this.#inFlight.set(item.id, null);
    let persisted = false;
    try {
      await this.#ready;
      const candidate = candidateFromItem(item);
      const known = this.#settings.known() ?? CAPTURE_EVERYTHING;
      if (!shouldCapture(candidate, known).capture) return null;
      const name = candidate.fileName ?? fileNameFromUrl(candidate.url) ?? 'A download';

      if (!(await this.#siteAccess(candidate.url))) {
        this.#notices.warn(`${name} stayed in the browser. ${NO_SITE_ACCESS}`);
        return null;
      }

      // Recorded before pausing, so a restarted background can withdraw it and act on the answer.
      const handoverId = this.#newHandoverId();
      this.#inFlight.set(item.id, handoverId);
      await this.#persistInFlight();
      persisted = true;
      const result = await handOver(
        { downloadId: item.id, handoverId, candidate, cookieStoreId: item.cookieStoreId ?? null },
        {
          downloads: this.#deps.downloads,
          liveSettings: () => this.#judgingSettings(),
          addDownload: (download) => this.#app.addDownload(download),
          withdraw: (handoverId) => this.#app.withdraw(handoverId),
          cookieHeader: (url, storeId, context) => this.#deps.cookieHeader(url, storeId, context),
          userAgent: this.#deps.userAgent,
        },
      );
      this.#notices.handover(result, name);
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
    const plan = planMenuClick(click, tab);
    switch (plan.kind) {
      case 'failed':
        return 'failed';
      case 'private_window':
        this.#notices.warn(MENU_PRIVATE_WINDOW);
        return 'private_window';
      case 'browser':
        return this.#browserFallback(plan.url, plan.message);
      case 'wirecatch':
        return this.#menuHandover(plan.url, plan.name, click, tab);
    }
  }

  async #menuHandover(
    url: string,
    name: string,
    click: MenuClick,
    tab: MenuTab | undefined,
  ): Promise<MenuResult> {
    if (!(await this.#siteAccess(url))) {
      return this.#browserFallback(
        url,
        `The browser is downloading ${name} instead. ${NO_SITE_ACCESS}`,
      );
    }
    await this.#ready;
    const handoverId = this.#newHandoverId();
    const page = click.frameUrl ?? click.pageUrl ?? null;
    const result = await this.#app.submit(handoverId, {
      url,
      // What the browser itself would send (strict-origin-when-cross-origin), never more.
      referrer: refererFor(url, page),
      page_url: click.pageUrl ?? null,
      suggested_name: null,
      mime: null,
      size_hint: null,
      // Like following the link: a top-level navigation started by the frame it is in.
      cookies: await this.#deps.cookieHeader(url, tab?.cookieStoreId ?? null, {
        initiator: page,
        navigation: true,
      }),
      source: 'context_menu',
    });
    if (result.taken) return 'handed_over';
    if (result.uncertain) this.#notices.keptUncertain(handoverId, name);
    return this.#browserFallback(url, menuNotTaken(name, result.detail));
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
    if (!this.#app.connected || this.#settings.live === null) {
      // Connect (and learn the default queue) first, so "could not be reached" is certain:
      // nothing has been sent yet when this fails.
      try {
        await this.#app.fetchSettings();
      } catch {
        return { ok: false, message: MEDIA_UNREACHABLE };
      }
    }
    const result = await this.#app.submit(this.#newHandoverId(), {
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
      source: 'media_button',
      media: { kind: target.kind, title: request.title },
    });
    return mediaAnswer(result);
  }

  async #browserFallback(url: string, message: string): Promise<'browser' | 'failed'> {
    try {
      await this.#deps.downloads.start(url);
      this.#notices.warn(message);
      return 'browser';
    } catch {
      this.#notices.error(BROWSER_DOWNLOAD_FAILED);
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

  /**
   * The settings to judge a paused download by: the app's, connecting first if
   * needed; when the app cannot be reached in time, the cached copy (marked as
   * such, so a cached "capture off" still keeps the download in the browser).
   */
  async #judgingSettings(): Promise<JudgingSettings | null> {
    const { live } = this.#settings;
    if (live !== null && this.#app.connected) {
      return { settings: live, fromApp: true };
    }
    try {
      const settings = await this.#app.fetchSettings();
      if (settings !== null) return { settings, fromApp: true };
    } catch {
      // Unreachable: fall back to the cache below.
    }
    // Read after the wait: settings that came in meanwhile are the newest copy.
    const { cached } = this.#settings;
    return cached === null ? null : { settings: cached.settings, fromApp: false };
  }

  async #persistInFlight(): Promise<void> {
    try {
      const entries = [...this.#inFlight].flatMap(([id, handoverId]) =>
        handoverId === null ? [] : [{ id, handoverId }],
      );
      await this.#deps.inFlight?.save(entries);
    } catch {
      // Session storage unavailable: recovery after a restart is best effort.
    }
  }

  /**
   * Settles the handovers a previous run of the background script paused and never finished,
   * exactly as after a timeout: `withdraw_download` decides. `already_started` means the app has
   * it, so the browser copy goes; `withdrawn` or `not_found` means it does not, so the browser
   * resumes it; no answer, or one that cannot be read, keeps the browser copy with a warning
   * that the file may download twice.
   */
  async #recoverInFlight(entries: readonly InFlightEntry[]): Promise<void> {
    const left = entries.filter((entry) => !this.#inFlight.has(entry.id));
    // Kept in the persisted list until settled, so another restart picks them up again.
    for (const { id, handoverId } of left) this.#inFlight.set(id, handoverId);
    for (const entry of left) {
      try {
        await this.#recover(entry);
      } finally {
        this.#inFlight.delete(entry.id);
        await this.#persistInFlight();
      }
    }
  }

  async #recover(entry: InFlightEntry): Promise<void> {
    const { result, name } = await recoverHandover(entry, {
      downloads: this.#deps.downloads,
      // The app may need connecting, or starting, first: this is the handover the person started.
      withdraw: (handoverId) => this.#app.withdraw(handoverId, true),
    });
    this.#notices.recovered(result, name, entry.handoverId);
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
      case 'open_app':
        await this.#app.showWindow();
        return;
      case 'dismiss_notice':
        this.#notices.dismiss();
        return;
      case 'pause_all':
        await this.#app.call({ method: 'pause_all' });
        return;
      case 'resume_all':
        await this.#app.call({ method: 'resume_all' });
        return;
      case 'update_settings':
        await this.#app.updateSettings(command.patch);
        return;
    }
  }

  #onStatus(status: ConnectionStatus): void {
    if (status.state === 'connected') {
      void this.#app.refresh();
    } else {
      this.#settings.disconnected(
        status.state === 'unavailable' && status.problem.kind === 'app_closed',
      );
    }
    this.#publish();
  }

  #onEvent(event: Event): void {
    switch (event.type) {
      case 'extension_settings_changed':
        this.#settings.adopt(event.data);
        return;
      case 'queues_changed':
        this.#settings.adoptQueues(event.data);
        return;
    }
  }

  /**
   * The app answered a handover after this side gave up waiting: warn if the browser kept its
   * copy too. An answer that cannot be read may be a confirmation, so it counts as one.
   */
  #onLateAnswer(call: Call, outcome: Outcome | null): void {
    if (call.method !== 'add_download') return;
    if (outcome !== null) {
      if (outcome.status !== 'ok') return;
      const reply = outcome.value;
      if (reply.type !== 'download_added') return;
    }
    const name = call.params.suggested_name ?? fileNameFromUrl(call.params.url) ?? 'A download';
    this.#notices.confirmedLate(call.params.handover_id, name);
  }

  /** The app's settings, or the cached copy when the app is not connected; null before either is known. */
  knownSettings(): ExtensionSettings | null {
    return this.#settings.known();
  }

  /** Shows a message in the popup (and the badge), for answers that have nowhere else to go. */
  notify(tone: Notice['tone'], message: string): void {
    this.#notices.notify(tone, message);
  }

  #publish(): void {
    const snapshot = this.snapshot();
    this.#popups.publish(snapshot);
    this.#updateBadge(snapshot);
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
