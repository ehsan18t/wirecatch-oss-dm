/** What the popup needs to know about the connection to the app, and how to explain problems. */

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
 * Firefox: "No such native application ...".
 */
export function classifyDisconnect(message: string | null): ProblemKind {
  if (message === null) return 'disconnected';
  const text = message.toLowerCase();
  if (text.includes('native messaging host is forbidden')) return 'host_untrusted';
  if (
    text.includes('native messaging host not found') ||
    text.includes('no such native application') ||
    text.includes('failed to start native messaging host')
  ) {
    return 'host_missing';
  }
  return 'disconnected';
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
