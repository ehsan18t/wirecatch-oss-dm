import { PRODUCT_NAME } from '@wirecatch/product';
import type { OptionChange, PageOptions } from '@wirecatch/protocol';
import type { MediaView } from '../media/center';
import {
  isBackgroundMessage,
  type BackgroundMessage,
  type Channel,
  type PopupCommand,
  type PopupRequest,
  type Snapshot,
} from '../popup-port';
import { parseOptions, type OptionValue, type ParsedGroup, type ParsedOption } from '../options';
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

type Result = { ok: true; message?: string; page?: PageOptions } | { ok: false; message: string };

/** A change on its way to the app, shown at once. */
interface Pending {
  request: number;
  id: string;
  change: OptionChange;
}

function isValue(value: unknown): value is OptionValue {
  return typeof value === 'string' || typeof value === 'number' || typeof value === 'boolean';
}

/** `option` with a change still on its way applied, so the popup shows it at once. */
function withChange(option: ParsedOption, change: OptionChange): ParsedOption {
  return 'set' in change ? withValue(option, change.set) : withEdit(option, change.edit);
}

/** `option` holding `value`, when it is a value of its kind. */
function withValue(option: ParsedOption, value: unknown): ParsedOption {
  if (option.kind === 'toggle') return typeof value === 'boolean' ? { ...option, value } : option;
  if (option.kind === 'choice') return isValue(value) ? { ...option, value } : option;
  if (option.kind === 'number') return typeof value === 'number' ? { ...option, value } : option;
  if (option.kind === 'limit') {
    return value === null || typeof value === 'number' ? { ...option, value } : option;
  }
  return option;
}

/** A `choices` option with values added and removed. */
function withEdit(
  option: ParsedOption,
  edit: { add: readonly unknown[]; remove: readonly unknown[] },
): ParsedOption {
  if (option.kind !== 'choices') return option;
  const added = edit.add.filter(isValue);
  const removed = new Set(edit.remove.filter(isValue));
  const value = [...option.value, ...added.filter((v) => !option.value.includes(v))].filter(
    (v) => !removed.has(v),
  );
  return { ...option, value };
}

/** What a request gets when the background script went away before it answered. */
const RESTARTED: Result = {
  ok: false,
  message: 'The extension restarted. Close and reopen this popup.',
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
  #pending = $state<Pending[]>([]);

  /** The options to show: the app's, with changes still on their way applied on top; null while the app is not connected (D61, D279). */
  groups = $derived.by((): ParsedGroup[] | null => {
    const list = this.snapshot?.options ?? null;
    if (list === null) return null;
    return parseOptions(list).map((group) => ({
      ...group,
      options: group.options.map((option) =>
        this.#pending
          .filter((p) => p.id === option.id)
          .reduce((shown, p) => withChange(shown, p.change), option),
      ),
    }));
  });

  connected = $derived(this.snapshot?.status.state === 'connected');
  /** Options can be changed only while the app is connected and has sent them (D61). */
  editable = $derived(this.connected && (this.snapshot?.options ?? null) !== null);

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

  /** Sends a change of one option to the app. Resolves with true once the app accepted it. */
  async setOption(id: string, change: OptionChange): Promise<boolean> {
    const request = this.#requests.nextId;
    this.#pending = [...this.#pending, { request, id, change }];
    const result = await this.#send({ type: 'set_option', id, change });
    this.#pending = this.#pending.filter((p) => p.request !== request);
    return this.#settled(result);
  }

  /** Runs an action, about the page on `site` when it names one. Resolves with true once the app did it. */
  async invokeAction(id: string, site: string | null): Promise<boolean> {
    return this.#settled(await this.#send({ type: 'invoke_action', id, site }));
  }

  /** What to show about the page on `host`; null when the app cannot say now. */
  async pageOptions(host: string): Promise<PageOptions | null> {
    const result = await this.#send({ type: 'page_options', host });
    return result.ok ? (result.page ?? null) : null;
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

  dismissAlert(id: string): void {
    void this.#send({ type: 'dismiss_alert', id });
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
      [key]: { tone: 'busy', text: `Sending to ${PRODUCT_NAME}…` },
    };
    const result = await this.#send({ type: 'download_media', tabId: view.tabId, key });
    this.mediaStatus = {
      ...this.mediaStatus,
      [key]: result.ok
        ? { tone: 'success', text: result.message ?? `Sent to ${PRODUCT_NAME}.` }
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
        ? {
            ok: true,
            ...(message.message === undefined ? {} : { message: message.message }),
            ...(message.page === undefined ? {} : { page: message.page }),
          }
        : { ok: false, message: message.message },
    );
  }
}
