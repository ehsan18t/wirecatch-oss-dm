/**
 * Messages between the popup and the background script (inside the extension only;
 * everything that reaches the app uses `@wirecatch/protocol`).
 */
import {
  isMediaKind,
  isOptionChange,
  isOptionList,
  isPageOptions,
  type OptionChange,
  type OptionList,
  type PageOptions,
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

/** Name of the `runtime.connect` port the extension's popup opens to the background script. */
export const POPUP_PORT_NAME = 'wirecatch-popup';

/** What `runtime.onConnect` says about a port. */
export interface PortInfo {
  name: string;
  sender?: { id?: string; url?: string };
}

/**
 * True only for a port of the extension's own pages: the right name, this extension, and opened
 * by one of the extension's pages (`extensionBaseUrl` is `runtime.getURL('/')`). A content
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
export interface Alert {
  /** Names the alert for dismissing it; one about a browser download is named after it. */
  id: string;
  /** The browser download it is about; a newer alert about the same download replaces it. */
  downloadId: number | null;
  /** Cleared once the browser reports the download running or finished. */
  untilResumed: boolean;
  /** Asks the person to act (resume a download, or check for two copies): kept over others when the list is full. */
  urgent: boolean;
  tone: 'warning' | 'error';
  message: string;
  at: number;
}

/** Checks a alert read back from storage. */
export const isAlert = fields<Alert>({
  id: isString,
  downloadId: nullable(isNumber),
  untilResumed: isBoolean,
  urgent: isBoolean,
  tone: member<Alert['tone']>({ warning: true, error: true }),
  message: isString,
  at: isNumber,
});

/** Everything the popup shows, pushed whenever any part of it changes. */
export interface Snapshot {
  status: ConnectionStatus;
  /** The connected app's options for the popup; null while it is not connected (D61, D279). */
  options: OptionList | null;
  /** "Work in private windows" as far as it is known; unknown counts as off (D279). */
  privateWindows: boolean;
  /** Newest first. */
  alerts: Alert[];
}

export type PopupCommand =
  /** Change one of the app's options. */
  | { type: 'set_option'; id: string; change: OptionChange }
  /** Run an action, about the page on `site` when it names one. */
  | { type: 'invoke_action'; id: string; site: string | null }
  /** What the popup shows about the page on `host`; the answer carries it. */
  | { type: 'page_options'; host: string }
  /** Start Wirecatch if it is closed, and show its browser settings. */
  | { type: 'open_settings' }
  | { type: 'pause_all' }
  | { type: 'resume_all' }
  | { type: 'reconnect' }
  | { type: 'dismiss_alert'; id: string }
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
  | { type: 'result'; id: number; ok: true; message?: string; page?: PageOptions }
  | { type: 'result'; id: number; ok: false; message: string };

export type BackgroundMessage =
  { type: 'snapshot'; snapshot: Snapshot } | { type: 'media'; view: MediaView } | ResultMessage;

const isSnapshot = fields<Snapshot>({
  status: isConnectionStatus,
  options: nullable(isOptionList),
  privateWindows: isBoolean,
  alerts: arrayOf(isAlert),
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
      page: optional(isPageOptions),
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
  postMessage: (message: Out) => void;
  onMessage: { addListener: (listener: (message: In) => void) => void };
  onDisconnect: { addListener: (listener: () => void) => void };
}

const isTabId = refine(isNumber, (id) => Number.isInteger(id) && id >= 0);
type Command<T extends PopupCommand['type']> = Variant<PopupCommand, T>;

const isCommand = variants<PopupCommand>({
  set_option: fields<Command<'set_option'>>({
    type: literal('set_option'),
    id: isString,
    change: isOptionChange,
  }),
  invoke_action: fields<Command<'invoke_action'>>({
    type: literal('invoke_action'),
    id: isString,
    site: nullable(isString),
  }),
  page_options: fields<Command<'page_options'>>({
    type: literal('page_options'),
    host: isString,
  }),
  open_settings: fields<Command<'open_settings'>>({ type: literal('open_settings') }),
  pause_all: fields<Command<'pause_all'>>({ type: literal('pause_all') }),
  resume_all: fields<Command<'resume_all'>>({ type: literal('resume_all') }),
  reconnect: fields<Command<'reconnect'>>({ type: literal('reconnect') }),
  dismiss_alert: fields<Command<'dismiss_alert'>>({
    type: literal('dismiss_alert'),
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
