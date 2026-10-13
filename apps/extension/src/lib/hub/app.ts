import type {
  AddedDownload,
  Call,
  ExtensionSettings,
  ExtensionSettingsPatch,
  NewDownload,
  Reply,
  WithdrawOutcome,
} from '@wirecatch/protocol';
import { submit, type SubmitResult } from '../capture/handover';
import { log } from '../diag/log';
import type { NativeClient } from '../native/client';
import type { HubTiming } from './deps';
import type { KnownSettings } from './settings';

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

  /** Reads the settings and queues again after connecting. */
  async refresh(): Promise<void> {
    try {
      const timeout = this.#timing.requestTimeoutMs;
      const [settings, queues] = await Promise.all([
        this.#client.request({ method: 'get_extension_settings' }, timeout),
        this.#client.request({ method: 'list_queues' }, timeout),
      ]);
      this.#settings.refreshed(
        settings.type === 'extension_settings' ? settings.data : null,
        queues.type === 'queues' ? queues.data : null,
      );
    } catch (error) {
      // The status listener already reports a broken connection; a slow answer is retried on the next connection.
      log.warn('settings', 'Could not refresh the settings and queues', error);
    }
  }

  /** Shows the app's window. Sent even when the app is closed: a request starts it, and the app then shows its window. */
  async showWindow(): Promise<void> {
    await this.#client.request(
      { method: 'show_window' },
      this.#mayConnect(this.#timing.requestTimeoutMs),
    );
  }

  async updateSettings(patch: ExtensionSettingsPatch): Promise<void> {
    const reply = await this.call({ method: 'update_extension_settings', params: patch });
    if (reply.type === 'extension_settings') this.#settings.adopt(reply.data);
  }

  /** A popup call: only to a connected app, since none of them is worth starting a closed one for. */
  async call(call: Call): Promise<Reply> {
    if (!this.connected) {
      throw new Error('Wirecatch is not connected, so this cannot be done right now.');
    }
    return this.#client.request(call, this.#timing.requestTimeoutMs);
  }
}
