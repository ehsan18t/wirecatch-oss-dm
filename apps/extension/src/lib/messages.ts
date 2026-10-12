/**
 * Messages between the popup and the background script (inside the extension only;
 * everything that reaches the app uses `@wirecatch/protocol`).
 */
import type { ExtensionSettings, ExtensionSettingsPatch, QueueSummary } from '@wirecatch/protocol';
import type { MediaEntry, MediaView } from './media/center';
import { isConnectionStatus, type ConnectionStatus } from './native/status';
import { isExtensionSettings, isMediaKind, isQueueSummary } from './protocol';
import {
  arrayOf,
  isBoolean,
  isNumber,
  isString,
  literal,
  member,
  nullable,
  object,
  oneOf,
  optional,
  refine,
  variants,
  type Guard,
} from './validate';

/** Name of the `runtime.connect` port the popup opens to the background script. */
export const POPUP_PORT_NAME = 'wirecatch-popup';

/** What `runtime.onConnect` says about a port. */
export interface PortInfo {
  name: string;
  sender?: { id?: string; url?: string };
}

/**
 * True only for the popup's own port: the right name, this extension, and opened by
 * one of the extension's pages (`extensionBaseUrl` is `runtime.getURL('/')`). A content
 * script also belongs to this extension and can use the name, but runs on a web page,
 * so it cannot change settings or send downloads through the popup's commands.
 */
export function acceptsPopupPort(
  port: PortInfo,
  extensionId: string,
  extensionBaseUrl: string,
): boolean {
  return (
    port.name === POPUP_PORT_NAME &&
    port.sender?.id === extensionId &&
    port.sender.url?.startsWith(extensionBaseUrl) === true
  );
}

/** Something that happened in the background that the person should know about. */
export interface Notice {
  tone: 'warning' | 'error';
  message: string;
  at: number;
}

/** Checks a notice read back from storage. */
export const isNotice: Guard<Notice> = object({
  tone: literal('warning', 'error'),
  message: isString,
  at: isNumber,
});

/** Everything the popup shows, pushed whenever any part of it changes. */
export interface Snapshot {
  status: ConnectionStatus;
  /** The app's settings, or the last cached copy when the app cannot be reached; null before the first connection. */
  settings: ExtensionSettings | null;
  /** True when `settings` is the cached copy, shown read-only. */
  settingsFromCache: boolean;
  /** When the cached copy was saved (milliseconds since the epoch). */
  cachedAt: number | null;
  queues: QueueSummary[];
  notice: Notice | null;
}

export type PopupCommand =
  | { type: 'update_settings'; patch: ExtensionSettingsPatch }
  | { type: 'pause_all' }
  | { type: 'resume_all' }
  | { type: 'reconnect' }
  /** Start Wirecatch if it is closed, and show its window. */
  | { type: 'open_app' }
  | { type: 'dismiss_notice' }
  /** Start sending `media` messages for this tab's "Media on this page" list. */
  | { type: 'watch_media'; tabId: number }
  /** Send one item of that list to the app. */
  | { type: 'download_media'; tabId: number; key: string };

export interface PopupRequest {
  id: number;
  command: PopupCommand;
}

export type BackgroundMessage =
  | { type: 'snapshot'; snapshot: Snapshot }
  | { type: 'media'; view: MediaView }
  /** `message`, when present, is a sentence to show for the success (for example "Sent to Wirecatch."). */
  | { type: 'result'; id: number; ok: true; message?: string }
  | { type: 'result'; id: number; ok: false; message: string };

const isSnapshot: Guard<Snapshot> = object({
  status: isConnectionStatus,
  settings: nullable(isExtensionSettings),
  settingsFromCache: isBoolean,
  cachedAt: nullable(isNumber),
  queues: arrayOf(isQueueSummary),
  notice: nullable(isNotice),
});

const isMediaEntry: Guard<MediaEntry> = object({
  key: isString,
  kind: isMediaKind,
  title: isString,
  detail: isString,
  size: nullable(isNumber),
});

const isMediaView: Guard<MediaView> = object({
  tabId: isNumber,
  page: member<MediaView['page']>({ web: true, private: true, other: true }),
  entries: arrayOf(isMediaEntry),
});

/**
 * Checks a message from the background script. It ships with the popup, but what comes
 * through a port is still checked before it is shown, like every value from outside.
 */
export const isBackgroundMessage = variants<BackgroundMessage>({
  snapshot: object({ type: literal('snapshot'), snapshot: isSnapshot }),
  media: object({ type: literal('media'), view: isMediaView }),
  result: oneOf(
    object({
      type: literal('result'),
      id: isNumber,
      ok: literal(true),
      message: optional(isString),
    }),
    object({ type: literal('result'), id: isNumber, ok: literal(false), message: isString }),
  ),
});

/** The part of `runtime.Port` both sides use; `Out` is what this side sends, `In` what it receives. */
export interface Channel<Out, In> {
  postMessage(message: Out): void;
  onMessage: { addListener(listener: (message: In) => void): void };
  onDisconnect: { addListener(listener: () => void): void };
}

const isTabId = refine(isNumber, (id) => Number.isInteger(id) && id >= 0);
const isStringList = optional(nullable(arrayOf(isString)));

const isPatch: Guard<ExtensionSettingsPatch> = object({
  capture_enabled: optional(nullable(isBoolean)),
  ask_before_download: optional(nullable(isBoolean)),
  show_media_button: optional(nullable(isBoolean)),
  ignored_sites: isStringList,
  add_ignored_sites: isStringList,
  remove_ignored_sites: isStringList,
  capture_extensions: isStringList,
  min_capture_size: optional(nullable(isNumber)),
  default_queue: optional(nullable(isNumber)),
});

const isCommand: Guard<PopupCommand> = oneOf(
  object({ type: literal('update_settings'), patch: isPatch }),
  object({
    type: literal('pause_all', 'resume_all', 'reconnect', 'open_app', 'dismiss_notice'),
  }),
  object({ type: literal('watch_media'), tabId: isTabId }),
  object({ type: literal('download_media'), tabId: isTabId, key: isString }),
);

const isPopupRequest: Guard<PopupRequest> = object({ id: isNumber, command: isCommand });

/** Checks a message from the popup; null for anything else. */
export function asPopupRequest(raw: unknown): PopupRequest | null {
  return isPopupRequest(raw) ? raw : null;
}
