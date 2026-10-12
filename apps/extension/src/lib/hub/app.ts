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
import type { NativeClient } from '../native/client';
import { applyPatch } from '../protocol';
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

  async addDownload(download: NewDownload): Promise<AddedDownload> {
    const reply = await this.#client.request(
      { method: 'add_download', params: download },
      this.#timing.requestTimeoutMs,
    );
    if (reply.type !== 'download_added') {
      throw new Error('Wirecatch gave an unexpected answer.');
    }
    return reply.data;
  }

  async withdraw(handoverId: string): Promise<WithdrawOutcome> {
    const reply = await this.#client.request(
      { method: 'withdraw_download', params: { handover_id: handoverId } },
      this.#timing.withdrawTimeoutMs,
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
    } catch {
      // The status listener already reports a broken connection; a slow answer is retried on the next connection.
    }
  }

  /** Shows the app's window. Sent even when the app is closed: a request starts it, and the app then shows its window. */
  async showWindow(): Promise<void> {
    await this.#client.request({ method: 'show_window' }, this.#timing.requestTimeoutMs);
  }

  async updateSettings(patch: ExtensionSettingsPatch): Promise<void> {
    const reply = await this.call({ method: 'update_extension_settings', params: patch });
    const { live } = this.#settings;
    if (reply.type === 'extension_settings') {
      this.#settings.adopt(reply.data);
    } else if (live !== null) {
      // The app only said "done"; its `extension_settings_changed` event confirms the values.
      this.#settings.adopt(applyPatch(live, patch));
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
