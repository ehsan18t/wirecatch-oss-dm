import {
  asPopupRequest,
  type BackgroundMessage,
  type Channel,
  type PopupCommand,
  type Snapshot,
} from '../messages';
import type { MediaCommands } from './deps';
import { log } from '../diag/log';
import { describeError } from './notices';

export type PopupChannel = Channel<BackgroundMessage, unknown>;

export interface PopupsDeps {
  /** The popup's media list; without it the popup's media commands fail. */
  media: MediaCommands | undefined;
  /** Runs a command other than `watch_media`; resolves with a sentence to show when there is one. */
  run: (command: PopupCommand) => Promise<string | undefined>;
}

/** The open popups: each gets every snapshot, its own media list, and an answer to each command. */
export class Popups {
  readonly #channels = new Set<PopupChannel>();
  readonly #deps: PopupsDeps;

  constructor(deps: PopupsDeps) {
    this.#deps = deps;
  }

  add(channel: PopupChannel): void {
    this.#channels.add(channel);
    let stopWatching: (() => void) | null = null;
    channel.onDisconnect.addListener(() => {
      this.#channels.delete(channel);
      stopWatching?.();
    });
    channel.onMessage.addListener((raw) => {
      const request = asPopupRequest(raw);
      if (request === null) return;
      const { id, command } = request;
      if (command.type === 'watch_media') {
        // One list per popup: the active tab's.
        stopWatching?.();
        stopWatching =
          this.#deps.media?.watch(command.tabId, (view) =>
            this.#post(channel, { type: 'media', view }),
          ) ?? null;
        this.#post(channel, { type: 'result', id, ok: true });
        return;
      }
      void this.#deps.run(command).then(
        (message) =>
          this.#post(
            channel,
            message === undefined
              ? { type: 'result', id, ok: true }
              : { type: 'result', id, ok: true, message },
          ),
        (error: unknown) =>
          this.#post(channel, { type: 'result', id, ok: false, message: describeError(error) }),
      );
    });
  }

  /** Sends the snapshot to one popup, or to every open one. */
  publish(snapshot: Snapshot, to?: PopupChannel): void {
    const message: BackgroundMessage = { type: 'snapshot', snapshot };
    if (to !== undefined) this.#post(to, message);
    else for (const channel of this.#channels) this.#post(channel, message);
  }

  #post(channel: PopupChannel, message: BackgroundMessage): void {
    try {
      channel.postMessage(message);
    } catch (error) {
      // The popup closed between two updates.
      log.debug('hub', 'A popup closed before its update', error);
      this.#channels.delete(channel);
    }
  }
}
