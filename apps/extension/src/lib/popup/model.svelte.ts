import {
  applyBrowserControlsPatch,
  applyExtensionSettingsPatch,
  type BrowserControls,
  type BrowserControlsPatch,
  type ExtensionSettings,
  type ExtensionSettingsPatch,
} from '@wirecatch/protocol';
import type { MediaView } from '../media/center';
import {
  isBackgroundMessage,
  type BackgroundMessage,
  type Channel,
  type PopupCommand,
  type PopupRequest,
  type Snapshot,
} from '../messages';
import { patchOf, type SettingsChange } from '../protocol';
import { log } from '../diag/log';
import { Requests } from './requests';

export interface Feedback {
  tone: 'success' | 'error';
  text: string;
}

/** What happened to one item of the media list after its Download button was clicked. */
export interface MediaStatus {
  tone: 'busy' | 'success' | 'error';
  text: string;
}

type Result = { ok: true; message?: string } | { ok: false; message: string };

/** What a request gets when the background script went away before it answered. */
const RESTARTED: Result = {
  ok: false,
  message: 'The extension restarted. Close and reopen this window.',
};

/**
 * The popup's view of the background state. Setting changes show at once (an optimistic
 * overlay) and settle when the app confirms them; a refusal rolls them back.
 */
export class PopupModel {
  snapshot = $state<Snapshot | null>(null);
  feedback = $state<Feedback | null>(null);
  /** The active tab's "Media on this page" list; null until the background sends it. */
  media = $state<MediaView | null>(null);
  /** Per item key: the outcome of its Download button. */
  mediaStatus = $state<Record<string, MediaStatus>>({});
  #pending = $state<{ id: number; patch: ExtensionSettingsPatch }[]>([]);
  #pendingControls = $state<{ id: number; patch: BrowserControlsPatch }[]>([]);

  /** The settings to show: the app's values with changes still on their way applied on top. */
  settings = $derived.by((): ExtensionSettings | null => {
    const base = this.snapshot?.settings ?? null;
    if (base === null) return null;
    return this.#pending.reduce((acc, p) => applyExtensionSettingsPatch(acc, p.patch), base);
  });

  /** The app's own settings the popup shows, with changes still on their way applied on top; null for an app from before them. */
  controls = $derived.by((): BrowserControls | null => {
    const base = this.snapshot?.controls ?? null;
    if (base === null) return null;
    return this.#pendingControls.reduce((acc, p) => applyBrowserControlsPatch(acc, p.patch), base);
  });

  connected = $derived(this.snapshot?.status.state === 'connected');
  /** Settings can be changed only while the app is connected and has sent them (D61). */
  editable = $derived(this.connected && (this.snapshot?.settings ?? null) !== null);

  readonly #channel: Channel<PopupRequest, unknown>;
  readonly #requests = new Requests<Result>();
  /** Pages waiting for what they show first; see `loaded`. */
  #waiting: { media: boolean; resolve: () => void }[] = [];

  constructor(channel: Channel<PopupRequest, unknown>) {
    this.#channel = channel;
    channel.onMessage.addListener((raw) => {
      if (isBackgroundMessage(raw)) this.#receive(raw);
    });
    channel.onDisconnect.addListener(() => {
      this.#requests.settleAll(RESTARTED);
    });
  }

  /**
   * Resolves once what a page shows first is in: the snapshot, and with `media` the active tab's
   * media list as well. A page mounts after it, so it opens at its final size.
   */
  loaded(media: boolean): Promise<void> {
    return new Promise((resolve) => {
      this.#waiting.push({ media, resolve });
      this.#settleWaiting();
    });
  }

  #settleWaiting(): void {
    if (this.snapshot === null) return;
    const ready = this.#waiting.filter((w) => !w.media || this.media !== null);
    this.#waiting = this.#waiting.filter((w) => !ready.includes(w));
    for (const w of ready) w.resolve();
  }

  /** Sends a settings change to the app. Resolves with true once the app accepted it. */
  async update(changes: SettingsChange): Promise<boolean> {
    const patch = patchOf(changes);
    const id = this.#requests.nextId;
    this.#pending = [...this.#pending, { id, patch }];
    const result = await this.#send({ type: 'update_settings', patch });
    this.#pending = this.#pending.filter((p) => p.id !== id);
    return this.#settled(result);
  }

  /** Sends a change of the app's own settings the popup shows. Resolves with true once the app accepted it. */
  async updateControls(patch: BrowserControlsPatch): Promise<boolean> {
    const id = this.#requests.nextId;
    this.#pendingControls = [...this.#pendingControls, { id, patch }];
    const result = await this.#send({ type: 'update_controls', patch });
    this.#pendingControls = this.#pendingControls.filter((p) => p.id !== id);
    return this.#settled(result);
  }

  #settled(result: Result): boolean {
    if (result.ok) return true;
    this.feedback = { tone: 'error', text: `That change was not saved. ${result.message}` };
    return false;
  }

  /** Starts Wirecatch if it is closed and shows its browser settings. */
  async openSettings(): Promise<boolean> {
    const result = await this.#send({ type: 'open_settings' });
    if (!result.ok) this.feedback = { tone: 'error', text: result.message };
    return result.ok;
  }

  async pauseAll(): Promise<void> {
    await this.#simple({ type: 'pause_all' }, 'All downloads are paused.');
  }

  async resumeAll(): Promise<void> {
    await this.#simple({ type: 'resume_all' }, 'All downloads are resuming.');
  }

  reconnect(): void {
    this.feedback = null;
    void this.#send({ type: 'reconnect' });
  }

  /** Starts Wirecatch if it is closed and shows its window. */
  async openApp(): Promise<void> {
    await this.#simple({ type: 'open_app' }, 'Wirecatch is open.');
  }

  dismissNotice(id: string): void {
    void this.#send({ type: 'dismiss_notice', id });
  }

  /** Asks for the media list of a tab (the active one), kept current while the popup is open. */
  watchMedia(tabId: number): void {
    void this.#send({ type: 'watch_media', tabId });
  }

  /** Sends one item of the media list to the app; its outcome shows in `mediaStatus`. */
  async downloadMedia(key: string): Promise<void> {
    const view = this.media;
    if (view === null || this.mediaStatus[key]?.tone === 'busy') return;
    this.mediaStatus = {
      ...this.mediaStatus,
      [key]: { tone: 'busy', text: 'Sending to Wirecatch…' },
    };
    const result = await this.#send({ type: 'download_media', tabId: view.tabId, key });
    this.mediaStatus = {
      ...this.mediaStatus,
      [key]: result.ok
        ? { tone: 'success', text: result.message ?? 'Sent to Wirecatch.' }
        : { tone: 'error', text: result.message },
    };
  }

  async #simple(command: PopupCommand, success: string): Promise<void> {
    const result = await this.#send(command);
    this.feedback = result.ok
      ? { tone: 'success', text: success }
      : { tone: 'error', text: result.message };
  }

  #send(command: PopupCommand): Promise<Result> {
    const { id, result } = this.#requests.start();
    try {
      this.#channel.postMessage({ id, command });
    } catch (error) {
      log.warn('popup', 'Could not reach the background', error);
      this.#requests.settle(id, RESTARTED);
    }
    return result;
  }

  #receive(message: BackgroundMessage): void {
    if (message.type === 'snapshot') {
      this.snapshot = message.snapshot;
      this.#settleWaiting();
      return;
    }
    if (message.type === 'media') {
      this.media = message.view;
      this.#settleWaiting();
      return;
    }
    this.#requests.settle(
      message.id,
      message.ok
        ? message.message === undefined
          ? { ok: true }
          : { ok: true, message: message.message }
        : { ok: false, message: message.message },
    );
  }
}
