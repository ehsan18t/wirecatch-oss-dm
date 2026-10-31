import {
  DEFAULT_EXTENSION_SETTINGS,
  type Call,
  type Event,
  type Outcome,
} from '@wirecatch/protocol';
import { candidateFromItem, type DownloadItemLike } from '../capture/candidate';
import { handOver, readBrowserCopy, type HandoverResult } from '../capture/handover';
import { shouldCapture } from '../capture/should-capture';
import { cookieStoreOf } from '../cookies';
import { fallback, log } from '../diag/log';
import { fileNameFromUrl } from '../file-types';
import type { Alert, PopupCommand, Snapshot } from '../messages';
import type { NativeClient } from '../native/client';
import type { ConnectionStatus } from '../native/status';
import { refererFor } from '../referrer';
import { appSide, isLocalCommand, type AppCalls, type AppSide } from './app';
import type { HubDeps, InFlightEntry, MenuClick, MenuResult, MenuTab } from './deps';
import { planMenuClick } from './menu';
import { AlertBoard } from './alert-board';
import {
  BROWSER_DOWNLOAD_FAILED,
  MENU_PRIVATE_WINDOW,
  NO_SITE_ACCESS,
  menuNotTaken,
} from './alerts';
import { Popups, type PopupChannel } from './popups';
import { recoverHandover } from './recovery';
import { hasAccess, newHandoverId } from './media-sender';
import type { KnownSettings } from './settings';

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
  readonly #alerts: AlertBoard;

  /** `side` is shared with the media side and the notices; tests that need neither leave it out. */
  constructor(deps: HubDeps, side: AppSide = appSide(deps)) {
    this.#deps = deps;
    this.#now = deps.now ?? Date.now;
    this.#settings = side.settings;
    this.#app = side.app;
    this.#settings.onChange(() => this.#publish());
    this.#alerts = new AlertBoard({
      now: this.#now,
      store: deps.alert,
      changed: () => this.#publish(),
      readCopy: (id) => readBrowserCopy(deps.downloads, id),
    });
    this.#popups = new Popups({ media: deps.media, run: (command) => this.#runCommand(command) });
    this.#ready = new Promise((resolve) => {
      this.#markReady = resolve;
    });
  }

  get #client(): NativeClient {
    return this.#deps.client;
  }

  /** Resolves once the session copy of the settings is loaded, so downloads are never judged without it. */
  get ready(): Promise<void> {
    return this.#ready;
  }

  /**
   * Loads the session copy of the settings, connects to the app if it was running when last seen, and resumes downloads a previous run left paused.
   *
   * The background wakes for every web response and tab change. While the app is closed, connecting each time would start a bridge process only to learn that again, so it waits until something needs the app: a request (which starts it) or the popup (which shows whether it runs).
   */
  async start(): Promise<void> {
    this.#client.onStatus((status) => this.#onStatus(status));
    this.#client.onEvent((event) => this.#onEvent(event));
    this.#client.onLateAnswer((call, outcome) => this.#onLateAnswer(call, outcome));
    // All are read before `ready`, so no new download can overwrite the list of paused ones first.
    const [cached, leftPaused] = await Promise.all([
      this.#deps.cache
        .load()
        .catch(fallback('settings', 'Could not read the session copy of the settings', null)),
      this.#deps.inFlight
        ?.load()
        .catch(
          fallback('capture', 'Could not read the downloads an earlier run left paused', []),
        ) ?? Promise.resolve([]),
      this.#alerts.load(),
    ]);
    this.#settings.loaded(cached);
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
      alerts: [...this.#alerts.current],
    };
  }

  /** Connects an open popup: it gets a snapshot once the session copy is loaded, and after every change. */
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
      const known = this.#settings.known() ?? DEFAULT_EXTENSION_SETTINGS;
      if (!shouldCapture(candidate, known).capture) return null;
      const name = candidate.fileName ?? fileNameFromUrl(candidate.url) ?? 'A download';

      if (!(await hasAccess(this.#deps, candidate.url))) {
        this.#alerts.warn(`${name} stayed in the browser. ${NO_SITE_ACCESS}`);
        return null;
      }

      // Recorded before pausing, so a restarted background can withdraw it and act on the answer.
      const handoverId = newHandoverId(this.#deps);
      this.#inFlight.set(item.id, handoverId);
      await this.#persistInFlight();
      persisted = true;
      const result = await handOver(
        { downloadId: item.id, handoverId, candidate, cookieStore: cookieStoreOf(item) },
        {
          downloads: this.#deps.downloads,
          liveSettings: () => this.#app.judgingSettings(),
          addDownload: (download) => this.#app.addDownload(download),
          withdraw: (handoverId) => this.#app.withdraw(handoverId),
          cookieHeader: (url, store, context) => this.#deps.cookieHeader(url, store, context),
          userAgent: this.#deps.userAgent,
        },
      );
      this.#alerts.handover(result, name, item.id);
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
    const privateWindows = tab?.incognito === true && (await this.#app.privateWindowsOn());
    const plan = planMenuClick(click, tab, privateWindows);
    switch (plan.kind) {
      case 'failed':
        return 'failed';
      case 'private_window':
        this.#alerts.warn(MENU_PRIVATE_WINDOW);
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
    if (!(await hasAccess(this.#deps, url))) {
      return this.#browserFallback(
        url,
        `The browser is downloading ${name} instead. ${NO_SITE_ACCESS}`,
      );
    }
    await this.#ready;
    const handoverId = newHandoverId(this.#deps);
    const page = click.frameUrl ?? click.pageUrl ?? null;
    const result = await this.#app.submit(handoverId, {
      url,
      // What the browser itself would send (strict-origin-when-cross-origin), never more.
      referrer: refererFor(url, page),
      page_url: click.pageUrl ?? null,
      // The document the link is in chose it, a frame from another site included.
      ...(page === null ? {} : { frame_url: page }),
      suggested_name: null,
      mime: null,
      size_hint: null,
      // Like following the link: a top-level navigation started by the frame it is in.
      cookies: await this.#deps.cookieHeader(url, cookieStoreOf(tab ?? {}), {
        initiator: page,
        navigation: true,
      }),
      source: 'context_menu',
    });
    if (result.taken) return 'handed_over';
    if (!result.uncertain) return this.#browserFallback(url, menuNotTaken(name, result.detail));
    const fallback = await this.#browserFallback(url, null);
    if (fallback === 'browser') this.#alerts.uncertain(handoverId, name, null);
    return fallback;
  }

  /**
   * Hands media to the app (from the media button or the popup). Uses the same
   * handover as downloads: a fresh handover id, and a withdraw when the answer is
   * lost. There is no browser download to keep, so when the app cannot be reached
   * the answer says to open Wirecatch instead of failing quietly.
   */
  async #browserFallback(url: string, message: string | null): Promise<'browser' | 'failed'> {
    try {
      await this.#deps.downloads.start(url);
      if (message !== null) this.#alerts.warn(message);
      return 'browser';
    } catch (error) {
      log.error('hub', 'The browser could not start the download either', error);
      this.#alerts.error(BROWSER_DOWNLOAD_FAILED);
      return 'failed';
    }
  }

  async #persistInFlight(): Promise<void> {
    try {
      const entries = [...this.#inFlight].flatMap(([id, handoverId]) =>
        handoverId === null ? [] : [{ id, handoverId }],
      );
      await this.#deps.inFlight?.save(entries);
    } catch (error) {
      // Recovery after a restart is best effort; this download is still handled now.
      log.warn('capture', 'Could not save the downloads waiting paused', error);
    }
  }

  /** Settles the handovers a previous run of the background script never finished (see `recoverHandover`). */
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
    const recovered = await recoverHandover(entry, {
      downloads: this.#deps.downloads,
      // The app may need connecting, or starting, first: this is the handover the person started.
      withdraw: (handoverId) => this.#app.withdraw(handoverId, true),
    });
    this.#alerts.recovered(recovered, entry.id);
  }

  /** Runs a popup command; resolves with a sentence to show when there is one. */
  async #runCommand(command: PopupCommand): Promise<string | undefined> {
    if (!isLocalCommand(command)) {
      await this.#app.run(command);
      return undefined;
    }
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
      case 'dismiss_alert':
        this.#alerts.dismiss(command.id);
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
      case 'browser_controls_changed':
        this.#settings.adoptControls(event.data);
        return;
      case 'notice_requested':
      case 'notice_withdrawn':
        void this.#deps.notices?.follow(event);
    }
  }

  /**
   * The app answered `add_download` after this side gave up waiting. `outcome` is null for an
   * answer that cannot be read and was not marked as an error, which may be a confirmation.
   */
  #onLateAnswer(call: Call, outcome: Outcome | null): void {
    if (call.method !== 'add_download') return;
    if (outcome !== null) {
      if (outcome.status !== 'ok') return;
      const reply = outcome.value;
      if (reply.type !== 'download_added') return;
    }
    const name = call.params.suggested_name ?? fileNameFromUrl(call.params.url) ?? 'A download';
    this.#alerts.confirmedLate(call.params.handover_id, name, outcome !== null);
  }

  /** The browser reports a download running or finished, so a alert asking to resume it is done. */
  onDownloadMoved(downloadId: number): void {
    this.#alerts.downloadMoved(downloadId);
  }

  /** Shows a message in the popup (and the badge), for answers that have nowhere else to go. */
  notify(tone: Alert['tone'], message: string): void {
    this.#alerts.notify(tone, message);
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
    } else if (snapshot.alerts.length > 0) {
      badge.set('!', 'Wirecatch has alerts for you. Click to see them.');
    } else {
      badge.set('', 'Wirecatch');
    }
  }
}
