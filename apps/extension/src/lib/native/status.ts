/** What the popup needs to know about the connection to the app, and how to explain problems. */
import {
  isNumber,
  isString,
  isUnknown,
  literal,
  member,
  nullable,
  object,
  variants,
} from '../validate';

export type ProblemKind =
  /** The browser could not find or start the native helper (the bridge): not installed or not registered. */
  | 'host_missing'
  /**
   * The bridge is registered, but its manifest does not list this extension's id, so the browser
   * refuses to start it. Reinstalling Wirecatch does not help; only the trusted extension or a
   * Wirecatch that trusts this id does.
   */
  | 'host_untrusted'
  /** The bridge runs but could not reach or start the app. */
  | 'app_unavailable'
  /** The app is closed, and nothing has asked for it since: the extension does not start it on its own. */
  | 'app_closed'
  /** The app speaks a different protocol version. */
  | 'version_mismatch'
  /** The connection broke for another reason. */
  | 'disconnected';

export interface Problem {
  kind: ProblemKind;
  /** Technical detail from the browser or the bridge, shown under the explanation. */
  detail: string | null;
}

export type ConnectionStatus =
  | { state: 'connecting' }
  | { state: 'connected'; appVersion: string }
  | { state: 'unavailable'; problem: Problem; retryAt: number | null };

const isProblemKind = member<ProblemKind>({
  host_missing: true,
  host_untrusted: true,
  app_unavailable: true,
  app_closed: true,
  version_mismatch: true,
  disconnected: true,
});

/** Checks a connection status the popup receives from the background. */
export const isConnectionStatus = variants<ConnectionStatus, 'state'>({
  connecting: object({ state: literal('connecting') }),
  connected: object({ state: literal('connected'), appVersion: isString }),
  unavailable: object({
    state: literal('unavailable'),
    problem: object({ kind: isProblemKind, detail: nullable(isString) }),
    retryAt: nullable(isNumber),
  }),
});

/**
 * Sorts the browser's disconnect message into a problem the person can act on.
 *
 * Chromium (the constants in extensions/browser/api/messaging/native_message_host.cc):
 * - "Specified native messaging host not found.": no manifest is registered under the name.
 * - "Failed to start native messaging host.": a manifest is registered but its program is gone.
 * - "Access to the specified native messaging host is forbidden.": the manifest's
 *   `allowed_origins` does not list this extension.
 * - "Native host has exited." and "Error when communicating with the native messaging host.":
 *   the bridge started and then stopped (for example because the app closed) or broke the protocol.
 *
 * Firefox (toolkit/components/extensions/NativeMessaging.sys.mjs and NativeManifests.sys.mjs,
 * and toolkit/modules/subprocess), read from `port.error` because Firefox does not set
 * `runtime.lastError` for ports:
 * - "No such native application app.wirecatch.bridge": deliberately the same message for no
 *   manifest, an unreadable manifest, and a manifest whose `allowed_extensions` leaves this add-on
 *   out (that reason only goes to the browser console). Our Firefox add-on always has the id every
 *   bridge lists, so for us it means Wirecatch is not installed or not registered.
 * - 'File at path "..." does not exist, or is not executable' (Linux and macOS), '... or is not
 *   a normal file' (Windows) and "Executable not found: ...": registered, but the program is gone.
 * - No error at all (`port.error` null) when the bridge exits, like Chromium's "Native host has
 *   exited.".
 * - "This extension does not have permission to use native ...": what Firefox logs when the id is
 *   not allowed. Current versions keep it in the console, but a fork or an older version that
 *   passes it on means the same as Chromium's "forbidden".
 */
export function classifyDisconnect(message: string | null): ProblemKind {
  if (message === null) return 'disconnected';
  const text = message.toLowerCase();
  if (
    text.includes('native messaging host is forbidden') ||
    text.includes('does not have permission to use native')
  ) {
    return 'host_untrusted';
  }
  if (
    text.includes('native messaging host not found') ||
    text.includes('no such native application') ||
    text.includes('failed to start native messaging host') ||
    /^file at path ".*" does not exist, or is not /.test(text) ||
    text.startsWith('executable not found: ')
  ) {
    return 'host_missing';
  }
  return 'disconnected';
}

/**
 * Why a native port closed, in the shape each browser reports it: Chromium sets
 * `runtime.lastError` while the disconnect listener runs, Firefox sets `port.error` (an Error,
 * or null when the host simply exited). Anything else counts as no reason.
 */
export function disconnectMessage(lastError: unknown, port: unknown): string | null {
  return messageOf(lastError) ?? (hasError(port) ? messageOf(port.error) : null);
}

const hasError = object({ error: isUnknown });

const hasMessage = object({ message: isString });

function messageOf(value: unknown): string | null {
  return hasMessage(value) && value.message !== '' ? value.message : null;
}

export interface ProblemText {
  title: string;
  fix: string;
}

/** Plain-language explanation and fix for each problem, shown in the popup. */
export function describeProblem(kind: ProblemKind): ProblemText {
  switch (kind) {
    case 'host_missing':
      return {
        title: 'Wirecatch is not installed, or not set up for this browser.',
        fix: 'Install Wirecatch, or open it once so it can connect to this browser.',
      };
    case 'host_untrusted':
      return {
        title: 'This copy of the extension is not the one Wirecatch trusts.',
        fix: 'Install the extension from the Wirecatch website or store, or update Wirecatch.',
      };
    case 'app_unavailable':
      return {
        title: 'The Wirecatch app could not be started.',
        fix: 'Open the Wirecatch app yourself, then click Try again.',
      };
    case 'app_closed':
      return {
        title: 'Wirecatch is closed.',
        fix: 'It opens when the browser has a download for it. You can also open it now.',
      };
    case 'version_mismatch':
      return {
        title: 'This extension and the Wirecatch app are different versions.',
        fix: 'Update the Wirecatch app and this extension to their latest versions.',
      };
    case 'disconnected':
      return {
        title: 'The connection to Wirecatch was lost.',
        fix: 'Wirecatch reconnects by itself. If this keeps happening, restart the Wirecatch app.',
      };
  }
}
