/**
 * Messages between the popup and the background script (inside the extension only;
 * everything that reaches the app uses `@wirecatch/protocol`).
 */
import type { ExtensionSettings, ExtensionSettingsPatch, QueueSummary } from '@wirecatch/protocol';
import type { ConnectionStatus } from './native/status';

/** Name of the `runtime.connect` port the popup opens to the background script. */
export const POPUP_PORT_NAME = 'wirecatch-popup';

/** Something that happened in the background that the person should know about. */
export interface Notice {
  tone: 'warning' | 'error';
  message: string;
  at: number;
}

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
  | { type: 'dismiss_notice' };

export interface PopupRequest {
  id: number;
  command: PopupCommand;
}

export type BackgroundMessage =
  | { type: 'snapshot'; snapshot: Snapshot }
  | { type: 'result'; id: number; ok: true }
  | { type: 'result'; id: number; ok: false; message: string };

/** The part of `runtime.Port` both sides use; `Out` is what this side sends, `In` what it receives. */
export interface Channel<Out, In> {
  postMessage(message: Out): void;
  onMessage: { addListener(listener: (message: In) => void): void };
  onDisconnect: { addListener(listener: () => void): void };
}

const COMMAND_TYPES = new Set<string>([
  'update_settings',
  'pause_all',
  'resume_all',
  'reconnect',
  'dismiss_notice',
]);

/** Checks the outer shape of a message from the popup. */
export function asPopupRequest(raw: unknown): PopupRequest | null {
  if (typeof raw !== 'object' || raw === null) return null;
  const { id, command } = raw as { id?: unknown; command?: unknown };
  if (typeof id !== 'number' || typeof command !== 'object' || command === null) return null;
  const type = (command as { type?: unknown }).type;
  if (typeof type !== 'string' || !COMMAND_TYPES.has(type)) return null;
  if (type === 'update_settings') {
    const patch = (command as { patch?: unknown }).patch;
    if (typeof patch !== 'object' || patch === null) return null;
  }
  return raw as PopupRequest;
}
