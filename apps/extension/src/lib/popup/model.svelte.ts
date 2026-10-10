import type { ExtensionSettings, ExtensionSettingsPatch } from '@wirecatch/protocol';
import type { BackgroundMessage, Channel, PopupCommand, PopupRequest, Snapshot } from '../messages';
import { applyPatch, patchOf } from '../protocol';

export interface Feedback {
  tone: 'success' | 'error';
  text: string;
}

function isBackgroundMessage(raw: unknown): raw is BackgroundMessage {
  if (typeof raw !== 'object' || raw === null) return false;
  const type = (raw as { type?: unknown }).type;
  return type === 'snapshot' || type === 'result';
}

/**
 * The popup's view of the background state. Setting changes show at once (an
 * optimistic overlay) and settle when the app confirms them; a refusal rolls them back.
 */
export class PopupModel {
  snapshot = $state<Snapshot | null>(null);
  feedback = $state<Feedback | null>(null);
  #pending = $state<Array<{ id: number; patch: ExtensionSettingsPatch }>>([]);

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
  // Callbacks for requests on their way, not displayed state, so a plain Map is right.
  // eslint-disable-next-line svelte/prefer-svelte-reactivity
  readonly #waiting = new Map<number, (result: { ok: true } | { ok: false; message: string }) => void>();
  #nextId = 1;

  constructor(channel: Channel<PopupRequest, unknown>) {
    this.#channel = channel;
    channel.onMessage.addListener((raw) => {
      if (isBackgroundMessage(raw)) this.#receive(raw);
    });
    channel.onDisconnect.addListener(() => {
      for (const done of this.#waiting.values())
        done({ ok: false, message: 'The extension restarted. Close and reopen this window.' });
      this.#waiting.clear();
    });
  }

  /** Sends a settings change to the app. Resolves with true once the app accepted it. */
  async update(changes: Partial<ExtensionSettings>): Promise<boolean> {
    const patch = patchOf(changes);
    const id = this.#nextId;
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

  dismissNotice(): void {
    void this.#send({ type: 'dismiss_notice' });
  }

  async #simple(command: PopupCommand, success: string): Promise<void> {
    const result = await this.#send(command);
    this.feedback = result.ok ? { tone: 'success', text: success } : { tone: 'error', text: result.message };
  }

  #send(command: PopupCommand): Promise<{ ok: true } | { ok: false; message: string }> {
    const id = this.#nextId++;
    return new Promise((resolve) => {
      this.#waiting.set(id, resolve);
      try {
        this.#channel.postMessage({ id, command });
      } catch {
        this.#waiting.delete(id);
        resolve({ ok: false, message: 'The extension restarted. Close and reopen this window.' });
      }
    });
  }

  #receive(message: BackgroundMessage): void {
    if (message.type === 'snapshot') {
      this.snapshot = message.snapshot;
      return;
    }
    const done = this.#waiting.get(message.id);
    if (done === undefined) return;
    this.#waiting.delete(message.id);
    done(message.ok ? { ok: true } : { ok: false, message: message.message });
  }
}
