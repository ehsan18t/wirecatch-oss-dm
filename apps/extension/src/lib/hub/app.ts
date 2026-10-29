import type {
  AddedDownload,
  Call,
  ExtensionSettings,
  NewDownload,
  NoticeId,
  Reply,
  WithdrawOutcome,
} from '@wirecatch/protocol';
import { submit, type SubmitResult } from '../capture/handover';
import { log } from '../diag/log';
import type { PopupCommand } from '../messages';
import { RequestError, type NativeClient } from '../native/client';
import type { HubTiming } from './deps';
import type { KnownSettings } from './settings';

/** The popup's commands that the app carries out. */
export type AppCommand = Extract<
  PopupCommand,
  {
    type:
      | 'open_app'
      | 'open_settings'
      | 'pause_all'
      | 'resume_all'
      | 'update_settings'
      | 'update_controls';
  }
>;

/** The popup's commands the background carries out itself; the app carries out the rest (`AppCalls.run`). */
export type LocalCommand = Extract<
  PopupCommand,
  { type: 'watch_media' | 'download_media' | 'reconnect' | 'dismiss_alert' }
>;
const LOCAL_COMMANDS: ReadonlySet<PopupCommand['type']> = new Set<LocalCommand['type']>([
  'watch_media',
  'download_media',
  'reconnect',
  'dismiss_alert',
]);
export const isLocalCommand = (command: PopupCommand): command is LocalCommand =>
  LOCAL_COMMANDS.has(command.type);

/** The popup's two kinds of settings change. */
type SettingsUpdate = Extract<AppCommand, { type: 'update_settings' | 'update_controls' }>;

/** The fields of a `NewDownload` that differ between the context menu and media; `submit` fills in the rest. */
export type DownloadFields = Omit<
  NewDownload,
  'handover_id' | 'user_agent' | 'extra_headers' | 'queue'
>;

/** The calls the background makes to the app, each with its wait, keeping `KnownSettings` up to date with the answers. */
export class AppCalls {
  readonly #client: NativeClient;
  readonly #timing: HubTiming;
  readonly #settings: KnownSettings;
  readonly #userAgent: string | null;

  constructor(
    client: NativeClient,
    timing: HubTiming,
    settings: KnownSettings,
    userAgent: string | null,
  ) {
    this.#client = client;
    this.#timing = timing;
    this.#settings = settings;
    this.#userAgent = userAgent;
  }

  get connected(): boolean {
    return this.#client.status.state === 'connected';
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
      throw new Error('Wirecatch gave an unexpected answer.');
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
    if (reply.type !== 'withdrawn') throw new Error('Wirecatch gave an unexpected answer.');
    return reply.data;
  }

  /** Offers a download from this side (not a browser download) to the app, into its default queue. */
  submit(handoverId: string, fields: DownloadFields): Promise<SubmitResult> {
    return submit(
      {
        ...fields,
        handover_id: handoverId,
        user_agent: this.#userAgent,
        extra_headers: [],
        queue: this.#settings.live?.default_queue ?? null,
      },
      {
        addDownload: (download) => this.addDownload(download),
        withdraw: (id) => this.withdraw(id),
      },
    );
  }

  /**
   * Asks the app for its settings, connecting (and starting it) first if needed, and
   * adopts them. Rejects when the app cannot be reached in time; null when it gave
   * another answer.
   */
  async fetchSettings(): Promise<ExtensionSettings | null> {
    const reply = await this.#client.request(
      { method: 'get_extension_settings' },
      this.#timing.connectWaitMs,
    );
    if (reply.type !== 'extension_settings') return null;
    this.#settings.adopt(reply.data);
    return reply.data;
  }

  /** Reads the settings and queues again after connecting; the browser controls too, from an app that has them (D61). */
  async refresh(): Promise<void> {
    try {
      const timeout = this.#timing.requestTimeoutMs;
      const [settings, queues, controls] = await Promise.all([
        this.#client.request({ method: 'get_extension_settings' }, timeout),
        this.#client.request({ method: 'list_queues' }, timeout),
        this.#client.supports('get_browser_controls')
          ? this.#client.request({ method: 'get_browser_controls' }, timeout)
          : null,
      ]);
      this.#settings.refreshed(
        settings.type === 'extension_settings' ? settings.data : null,
        controls?.type === 'browser_controls' ? controls.data : null,
        queues.type === 'queues' ? queues.data : null,
      );
    } catch (error) {
      // The status listener already reports a broken connection; a slow answer is retried on the next connection.
      log.warn('settings', 'Could not refresh the settings and queues', error);
    }
  }

  /** Tells the app a note showed (D65). Only while connected: the note came from the app. */
  async noticeShown(id: NoticeId): Promise<void> {
    await this.#client.request(
      { method: 'notice_shown', params: { id } },
      this.#timing.requestTimeoutMs,
    );
  }

  /** Tells the app the person pressed one of a note's buttons (D65). */
  async noticePressed(id: NoticeId, button: number): Promise<void> {
    await this.#client.request(
      { method: 'notice_pressed', params: { id, button } },
      this.#timing.requestTimeoutMs,
    );
  }

  /** Shows the app's window. Sent even when the app is closed: a request starts it, and the app then shows its window. */
  async showWindow(): Promise<void> {
    await this.#client.request(
      { method: 'show_window' },
      this.#mayConnect(this.#timing.requestTimeoutMs),
    );
  }

  /** Shows the app's browser settings (D61), starting the app first like `showWindow`; an app from before them shows its window. */
  async showBrowserSettings(): Promise<void> {
    try {
      await this.#client.request(
        { method: 'show_browser_settings' },
        this.#mayConnect(this.#timing.requestTimeoutMs),
      );
    } catch (error) {
      const tooOld =
        error instanceof RequestError && error.protocolError?.code === 'unsupported_call';
      if (!tooOld) throw error;
      await this.showWindow();
    }
  }

  /** Carries out a popup command that the app does. */
  async run(command: AppCommand): Promise<void> {
    switch (command.type) {
      case 'open_app':
        await this.showWindow();
        return;
      case 'open_settings':
        await this.showBrowserSettings();
        return;
      case 'pause_all':
      case 'resume_all':
        await this.call({ method: command.type });
        return;
      case 'update_settings':
      case 'update_controls':
        await this.#update(command);
        return;
    }
  }

  /** A settings change from the popup: the extension's own settings, or the app's it shows (D61). */
  async #update(change: SettingsUpdate): Promise<void> {
    if (change.type === 'update_settings') {
      const reply = await this.call({ method: 'update_extension_settings', params: change.patch });
      if (reply.type === 'extension_settings') this.#settings.adopt(reply.data);
    } else {
      const reply = await this.call({ method: 'update_browser_controls', params: change.patch });
      if (reply.type === 'browser_controls') this.#settings.adoptControls(reply.data);
    }
  }

  /** A popup call: only to a connected app, since none of them is worth starting a closed one for. */
  async call(call: Call): Promise<Reply> {
    if (!this.connected) {
      throw new Error('Wirecatch is not connected, so this cannot be done right now.');
    }
    return this.#client.request(call, this.#timing.requestTimeoutMs);
  }
}
