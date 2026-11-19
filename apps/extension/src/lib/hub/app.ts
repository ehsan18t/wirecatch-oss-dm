import { PRODUCT_NAME } from '@wirecatch/product';
import type {
  AddedDownload,
  Call,
  CheckAnswer,
  CookiePlan,
  DownloadCheck,
  NewDownload,
  NoticeId,
  PageOptions,
  Reply,
  ShellConfig,
  WithdrawOutcome,
} from '@wirecatch/protocol';
import {
  newDownload,
  submit,
  type DownloadFields,
  type JudgingSettings,
  type SubmitResult,
} from '../capture/handover';
import type { DownloadCookies } from '../cookies';
import { log } from '../diag/log';
import { messageOf } from '../errors';
import type { PopupCommand } from '../popup-port';
import type { NativeClient } from '../native/client';
import { hubTiming, type HubDeps, type HubTiming, type PrivateSessionDep } from './deps';
import { KnownSettings } from './settings';

/** The app's reason for leaving a capture of a private session it already forgot (D280). */
const PRIVATE_SESSION_ENDED = 'private_session_ended';

/** The popup's commands that the app carries out. */
export type AppCommand = Extract<
  PopupCommand,
  { type: 'open_settings' | 'pause_all' | 'resume_all' | 'set_option' | 'invoke_action' }
>;

/** The popup's commands the background carries out itself, or answers with data; the app carries out the rest (`AppCalls.run`). */
export type LocalCommand = Extract<
  PopupCommand,
  { type: 'watch_media' | 'download_media' | 'reconnect' | 'dismiss_alert' | 'page_options' }
>;
const LOCAL_COMMANDS: ReadonlySet<PopupCommand['type']> = new Set<LocalCommand['type']>([
  'watch_media',
  'download_media',
  'reconnect',
  'dismiss_alert',
  'page_options',
]);
export const isLocalCommand = (command: PopupCommand): command is LocalCommand =>
  LOCAL_COMMANDS.has(command.type);

/** The calls the background makes to the app, each with its wait, keeping `KnownSettings` up to date with the answers. */
export class AppCalls {
  readonly #client: NativeClient;
  readonly #timing: HubTiming;
  readonly #settings: KnownSettings;
  readonly #userAgent: string | null;
  readonly #privateSession: PrivateSessionDep | undefined;

  constructor(
    client: NativeClient,
    timing: HubTiming,
    settings: KnownSettings,
    browser: { userAgent: string | null; privateSession?: PrivateSessionDep },
  ) {
    this.#client = client;
    this.#timing = timing;
    this.#settings = settings;
    this.#userAgent = browser.userAgent;
    this.#privateSession = browser.privateSession;
  }

  get connected(): boolean {
    return this.#client.status.state === 'connected';
  }

  get timing(): HubTiming {
    return this.#timing;
  }

  /** The wait for a call that connects first when the app is not connected, which may start it: as long as a hello may take, then `answerMs` for the answer. */
  #mayConnect(answerMs: number): number {
    return this.connected ? answerMs : this.#client.helloTimeoutMs + answerMs;
  }

  async addDownload(download: NewDownload): Promise<AddedDownload> {
    const reply = await this.#client.request(
      { method: 'add_download', params: download },
      this.#mayConnect(this.#timing.requestTimeoutMs),
    );
    if (reply.type !== 'download_added') {
      throw new Error(`${PRODUCT_NAME} gave an unexpected answer.`);
    }
    return reply.data;
  }

  /** Asks the app to forget a handover. `connecting`: the app may not be connected, or even running, so the wait also covers a full hello, which may start it; a shorter one would end a slow start as "no answer" and warn of two copies for nothing. */
  async withdraw(handoverId: string, connecting = false): Promise<WithdrawOutcome> {
    const { withdrawTimeoutMs } = this.#timing;
    const reply = await this.#client.request(
      { method: 'withdraw_download', params: { handover_id: handoverId } },
      connecting ? this.#mayConnect(withdrawTimeoutMs) : withdrawTimeoutMs,
    );
    if (reply.type !== 'withdrawn') throw new Error(`${PRODUCT_NAME} gave an unexpected answer.`);
    return reply.data;
  }

  /** Offers a download from this side (not a browser download) to the app, which picks its queue. */
  submit(handoverId: string, fields: DownloadFields): Promise<SubmitResult> {
    return submit(newDownload(handoverId, this.#userAgent, fields), {
      addDownload: (download) => this.addDownload(download),
      withdraw: (id) => this.withdraw(id),
    });
  }

  /**
   * Offers something the person picked (the menu, the media button) to the app: first asks
   * whether it takes it, with no cookies, then reads the cookies for the sites it named and hands
   * it over (D280). The person acted, so this may start the app.
   */
  async submitChecked(
    handoverId: string,
    check: DownloadCheck,
    fields: Omit<DownloadFields, 'cookies' | 'cookie_jar'>,
    cookies: (plan: CookiePlan) => Promise<DownloadCookies>,
  ): Promise<SubmitResult> {
    let answer: CheckAnswer;
    try {
      answer = await this.check(check, this.#mayConnect(this.#timing.requestTimeoutMs));
    } catch (error) {
      // Nothing with credentials was sent, so the app has nothing to withdraw.
      return { taken: false, reason: 'no_answer', detail: messageOf(error), uncertain: false };
    }
    if (answer.decision === 'leave') {
      return { taken: false, reason: 'refused', detail: null, uncertain: false };
    }
    return this.submit(handoverId, { ...fields, ...(await cookies(answer.cookies)) });
  }

  /**
   * Asks the app for the values the extension's checks use, connecting (and starting it) first
   * if needed, and adopts them. Rejects when the app cannot be reached within `timeoutMs`.
   */
  async fetchConfig(timeoutMs = this.#timing.connectWaitMs): Promise<ShellConfig | null> {
    const reply = await this.#client.request({ method: 'get_shell_config' }, timeoutMs);
    if (reply.type !== 'shell_config') return null;
    this.#settings.adopt(reply.data);
    return reply.data;
  }

  /** Asks whether the app takes a capture, before any cookie is read. Rejects when no answer comes within `timeoutMs`. */
  async check(check: DownloadCheck, timeoutMs: number): Promise<CheckAnswer> {
    const reply = await this.#client.request(
      { method: 'check_download', params: check },
      timeoutMs,
    );
    if (reply.type !== 'download_check') {
      throw new Error(`${PRODUCT_NAME} gave an unexpected answer.`);
    }
    const answer = reply.data;
    if (
      answer.decision === 'leave' &&
      answer.reason === PRIVATE_SESSION_ENDED &&
      check.private_session !== null
    ) {
      this.#privateSession?.renew?.(check.private_session);
    }
    return answer;
  }

  /** Reads the values and the options again after connecting. */
  async refresh(): Promise<void> {
    try {
      const timeout = this.#timing.requestTimeoutMs;
      const [config, options] = await Promise.all([
        this.#client.request({ method: 'get_shell_config' }, timeout),
        this.#client.request({ method: 'get_options' }, timeout),
      ]);
      if (config.type === 'shell_config') this.#settings.adopt(config.data);
      if (options.type === 'options') this.#settings.adoptOptions(options.data);
    } catch (error) {
      // The status listener already reports a broken connection; a slow answer is retried on the next connection.
      log.warn('settings', 'Could not read the options', error);
    }
  }

  /** What the popup shows about the page on `host`. */
  async pageOptions(host: string): Promise<PageOptions> {
    const reply = await this.call({ method: 'page_options', params: { host } });
    if (reply.type !== 'page_options')
      throw new Error(`${PRODUCT_NAME} gave an unexpected answer.`);
    return reply.data;
  }

  /** Tells a connected app that the private session `id` ended. An app not connected forgets it once no connection names it. */
  async privateSessionEnded(id: string): Promise<void> {
    if (!this.connected) return;
    await this.#client.request(
      { method: 'private_session_ended', params: { id } },
      this.#timing.requestTimeoutMs,
    );
  }

  /** Tells the app a note showed (D65). Only while connected: the note came from the app. */
  async noticeShown(id: NoticeId): Promise<void> {
    await this.#client.request(
      { method: 'notice_shown', params: { id } },
      this.#timing.requestTimeoutMs,
    );
  }

  /** Tells the app the person pressed one of a notice's buttons, or its text (D65). */
  async noticePressed(id: NoticeId, button: number | null): Promise<void> {
    await this.#client.request(
      { method: 'notice_pressed', params: { id, button } },
      this.#timing.requestTimeoutMs,
    );
  }

  /** Shows the app's browser settings (D61), starting the app first like any request. */
  async showBrowserSettings(): Promise<void> {
    await this.#client.request(
      { method: 'show_browser_settings' },
      this.#mayConnect(this.#timing.requestTimeoutMs),
    );
  }

  /** Carries out a popup command that the app does. */
  async run(command: AppCommand): Promise<void> {
    switch (command.type) {
      case 'open_settings':
        await this.showBrowserSettings();
        return;
      case 'pause_all':
      case 'resume_all':
        await this.call({ method: command.type });
        return;
      case 'set_option': {
        const reply = await this.call({
          method: 'set_option',
          params: { id: command.id, change: command.change },
        });
        if (reply.type === 'options') this.#settings.adoptOptions(reply.data);
        return;
      }
      case 'invoke_action':
        await this.call({
          method: 'invoke_action',
          params: { id: command.id, site: command.site },
        });
        return;
    }
  }

  /** A popup call: only to a connected app, since none of them is worth starting a closed one for. */
  async call(call: Call): Promise<Reply> {
    if (!this.connected) {
      throw new Error(`${PRODUCT_NAME} is not connected, so this cannot be done right now.`);
    }
    return this.#client.request(call, this.#timing.requestTimeoutMs);
  }

  /**
   * The values to judge a capture by: the app's, connecting first if needed and within
   * `timeoutMs`; when the app cannot be reached in time, the session copy, marked as such so
   * nothing is taken on it alone; null when neither is known.
   */
  async judgingSettings(timeoutMs = this.#timing.connectWaitMs): Promise<JudgingSettings | null> {
    const { live } = this.#settings;
    if (live !== null && this.connected) {
      return { settings: live, fromApp: true };
    }
    try {
      const settings = await this.fetchConfig(timeoutMs);
      if (settings !== null) return { settings, fromApp: true };
    } catch (error) {
      log.warn('settings', 'Could not reach the app; judging by the session copy', error);
    }
    // Read after the wait: values that came in meanwhile are the newest copy.
    const { cached } = this.#settings;
    return cached === null ? null : { settings: cached.settings, fromApp: false };
  }

  /** "Work in private windows" (D82) for something the person did, such as the menu: asked of the app when unknown, which may start it; an app that cannot be reached counts as off. */
  async privateWindowsOn(): Promise<boolean> {
    await this.#settings.ready;
    const settings = this.#settings.known() ?? (await this.judgingSettings())?.settings;
    return settings?.private_windows === true;
  }
}

/** The app's values and the calls to it, which the hub, the media side and the notices share. */
export interface AppSide {
  settings: KnownSettings;
  app: AppCalls;
}

export function appSide(
  deps: Pick<
    HubDeps,
    'client' | 'cache' | 'mediaButtonSetting' | 'timing' | 'userAgent' | 'privateSession'
  >,
): AppSide {
  const settings = new KnownSettings(deps);
  return {
    settings,
    app: new AppCalls(deps.client, hubTiming(deps.timing), settings, {
      userAgent: deps.userAgent,
      privateSession: deps.privateSession,
    }),
  };
}
