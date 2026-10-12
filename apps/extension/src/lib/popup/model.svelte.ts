import type { ExtensionSettings, ExtensionSettingsPatch } from '@wirecatch/protocol';
import type { MediaView } from '../media/center';
import type { BackgroundMessage, Channel, PopupCommand, PopupRequest, Snapshot } from '../messages';
import { applyPatch, patchOf, type SettingsChange } from '../protocol';
import { literal, object } from '../validate';
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
 * Checks the kind of a message from the background script, which is this extension's
 * own code; its fields are trusted, as the popup and the background ship together.
 */
const hasBackgroundType = object({ type: literal('snapshot', 'result', 'media') });

function isBackgroundMessage(raw: unknown): raw is BackgroundMessage {
  return hasBackgroundType(raw);
}

/**
 * The popup's view of the background state. Setting changes show at once (an
 * optimistic overlay) and settle when the app confirms them; a refusal rolls them back.
 */
export class PopupModel {
  snapshot = $state<Snapshot | null>(null);
  feedback = $state<Feedback | null>(null);
  /** The active tab's "Media on this page" list; null until the background sends it. */
  media = $state<MediaView | null>(null);
  /** Per item key: the outcome of its Download button. */
  mediaStatus = $state<Record<string, MediaStatus>>({});
  #pending = $state<{ id: number; patch: ExtensionSettingsPatch }[]>([]);

  /** The settings to show: the app's (or cached) values with changes still on their way applied on top. */
  settings = $derived.by((): ExtensionSettings | null => {
    const base = this.snapshot?.settings ?? null;
    if (base === null) return null;
    return this.#pending.reduce((acc, p) => applyPatch(acc, p.patch), base);
  });

  connected = $derived(this.snapshot?.status.state === 'connected');
  /** Settings can be changed only while connected and showing the app's own values. */
  editable = $derived(
    this.connected && this.snapshot?.settingsFromCache === false && this.snapshot.settings !== null,
  );

  readonly #channel: Channel<PopupRequest, unknown>;
  readonly #requests = new Requests<Result>();

  constructor(channel: Channel<PopupRequest, unknown>) {
    this.#channel = channel;
    channel.onMessage.addListener((raw) => {
      if (isBackgroundMessage(raw)) this.#receive(raw);
    });
    channel.onDisconnect.addListener(() => {
      this.#requests.settleAll(RESTARTED);
    });
  }

  /** Sends a settings change to the app. Resolves with true once the app accepted it. */
  async update(changes: SettingsChange): Promise<boolean> {
    const patch = patchOf(changes);
    const id = this.#requests.nextId;
    this.#pending = [...this.#pending, { id, patch }];
    const result = await this.#send({ type: 'update_settings', patch });
    this.#pending = this.#pending.filter((p) => p.id !== id);
    if (!result.ok) {
      this.feedback = { tone: 'error', text: `That change was not saved. ${result.message}` };
      return false;
    }
    return true;
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

  dismissNotice(): void {
    void this.#send({ type: 'dismiss_notice' });
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
    } catch {
      this.#requests.settle(id, RESTARTED);
    }
    return result;
  }

  #receive(message: BackgroundMessage): void {
    if (message.type === 'snapshot') {
      this.snapshot = message.snapshot;
      return;
    }
    if (message.type === 'media') {
      this.media = message.view;
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
