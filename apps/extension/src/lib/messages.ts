/**
 * Messages between the popup and the background script (inside the extension only;
 * everything that reaches the app uses `@wirecatch/protocol`).
 */
import {
  isExtensionSettings,
  isMediaKind,
  isQueueSummary,
  type ExtensionSettings,
  type ExtensionSettingsPatch,
  type QueueSummary,
} from '@wirecatch/protocol';
import type { MediaEntry, MediaView } from './media/center';
import { isConnectionStatus, type ConnectionStatus } from './native/status';
import {
  arrayOf,
  fields,
  isBoolean,
  isNumber,
  isString,
  literal,
  member,
  nullable,
  optional,
  refine,
  type Variant,
  variants,
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
  /** Names the notice for dismissing it; one about a browser download is named after it. */
  id: string;
  /** The browser download it is about; a newer notice about the same download replaces it. */
  downloadId: number | null;
  /** Cleared once the browser reports the download running or finished. */
  untilResumed: boolean;
  /** Asks the person to act (resume a download, or check for two copies): kept over others when the list is full. */
  urgent: boolean;
  tone: 'warning' | 'error';
  message: string;
  at: number;
}

/** Checks a notice read back from storage. */
export const isNotice = fields<Notice>({
  id: isString,
  downloadId: nullable(isNumber),
  untilResumed: isBoolean,
  urgent: isBoolean,
  tone: member<Notice['tone']>({ warning: true, error: true }),
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
  /** Newest first. */
  notices: Notice[];
}

export type PopupCommand =
  | { type: 'update_settings'; patch: ExtensionSettingsPatch }
  | { type: 'pause_all' }
  | { type: 'resume_all' }
  | { type: 'reconnect' }
  /** Start Wirecatch if it is closed, and show its window. */
  | { type: 'open_app' }
  | { type: 'dismiss_notice'; id: string }
  /** Start sending `media` messages for this tab's "Media on this page" list. */
  | { type: 'watch_media'; tabId: number }
  /** Send one item of that list to the app. */
  | { type: 'download_media'; tabId: number; key: string };

export interface PopupRequest {
  id: number;
  command: PopupCommand;
}

/** The answer to one `PopupRequest`. */
export type ResultMessage =
  /** `message`, when present, is a sentence to show for the success (for example "Sent to Wirecatch."). */
  | { type: 'result'; id: number; ok: true; message?: string }
  | { type: 'result'; id: number; ok: false; message: string };

export type BackgroundMessage =
  { type: 'snapshot'; snapshot: Snapshot } | { type: 'media'; view: MediaView } | ResultMessage;

const isSnapshot = fields<Snapshot>({
  status: isConnectionStatus,
  settings: nullable(isExtensionSettings),
  settingsFromCache: isBoolean,
  cachedAt: nullable(isNumber),
  queues: arrayOf(isQueueSummary),
  notices: arrayOf(isNotice),
});

const isMediaEntry = fields<MediaEntry>({
  key: isString,
  kind: isMediaKind,
  title: isString,
  detail: isString,
  size: nullable(isNumber),
});

const isMediaView = fields<MediaView>({
  tabId: isNumber,
  page: member<MediaView['page']>({ web: true, private: true, other: true }),
  entries: arrayOf(isMediaEntry),
});

/**
 * Checks a message from the background script. It ships with the popup, but what comes
 * through a port is still checked before it is shown, like every value from outside.
 */
export const isBackgroundMessage = variants<BackgroundMessage>({
  snapshot: fields<Variant<BackgroundMessage, 'snapshot'>>({
    type: literal('snapshot'),
    snapshot: isSnapshot,
  }),
  media: fields<Variant<BackgroundMessage, 'media'>>({ type: literal('media'), view: isMediaView }),
  result: variants<ResultMessage, 'ok'>({
    true: fields<Extract<ResultMessage, { ok: true }>>({
      type: literal('result'),
      id: isNumber,
      ok: literal(true),
      message: optional(isString),
    }),
    false: fields<Extract<ResultMessage, { ok: false }>>({
      type: literal('result'),
      id: isNumber,
      ok: literal(false),
      message: isString,
    }),
  }),
});

/** The part of `runtime.Port` both sides use; `Out` is what this side sends, `In` what it receives. */
export interface Channel<Out, In> {
  postMessage(message: Out): void;
  onMessage: { addListener(listener: (message: In) => void): void };
  onDisconnect: { addListener(listener: () => void): void };
}

const isTabId = refine(isNumber, (id) => Number.isInteger(id) && id >= 0);
/** A list field of a patch: a new list, null for no change, or left out. */
const isListChange = optional(nullable(arrayOf(isString)));

const isPatch = fields<ExtensionSettingsPatch>({
  capture_enabled: optional(nullable(isBoolean)),
  ask_before_download: optional(nullable(isBoolean)),
  show_media_button: optional(nullable(isBoolean)),
  ignored_sites: isListChange,
  add_ignored_sites: isListChange,
  remove_ignored_sites: isListChange,
  capture_extensions: isListChange,
  min_capture_size: optional(nullable(isNumber)),
  default_queue: optional(nullable(isNumber)),
});

type Command<T extends PopupCommand['type']> = Variant<PopupCommand, T>;

const isCommand = variants<PopupCommand>({
  update_settings: fields<Command<'update_settings'>>({
    type: literal('update_settings'),
    patch: isPatch,
  }),
  pause_all: fields<Command<'pause_all'>>({ type: literal('pause_all') }),
  resume_all: fields<Command<'resume_all'>>({ type: literal('resume_all') }),
  reconnect: fields<Command<'reconnect'>>({ type: literal('reconnect') }),
  open_app: fields<Command<'open_app'>>({ type: literal('open_app') }),
  dismiss_notice: fields<Command<'dismiss_notice'>>({
    type: literal('dismiss_notice'),
    id: isString,
  }),
  watch_media: fields<Command<'watch_media'>>({ type: literal('watch_media'), tabId: isTabId }),
  download_media: fields<Command<'download_media'>>({
    type: literal('download_media'),
    tabId: isTabId,
    key: isString,
  }),
});

const isPopupRequest = fields<PopupRequest>({ id: isNumber, command: isCommand });

/** Checks a message from the popup; null for anything else. */
export function asPopupRequest(raw: unknown): PopupRequest | null {
  return isPopupRequest(raw) ? raw : null;
}
