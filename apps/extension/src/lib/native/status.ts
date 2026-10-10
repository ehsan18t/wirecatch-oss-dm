/** What the popup needs to know about the connection to the app, and how to explain problems. */

export type ProblemKind =
  /** The browser could not find or start the native helper (the bridge). */
  | 'host_missing'
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
 * Chromium: "Specified native messaging host not found." or "Access to the specified
 * native messaging host is forbidden." Firefox: "No such native application ...".
 */
export function classifyDisconnect(message: string | null): ProblemKind {
  if (message === null) return 'disconnected';
  const text = message.toLowerCase();
  if (
    text.includes('not found') ||
    text.includes('no such native application') ||
    text.includes('is forbidden') ||
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
        title: 'Wirecatch is not set up for this browser.',
        fix: 'Install the Wirecatch app (or run "wirecatch-bridge install" if it is already installed), restart the browser, then click Try again.',
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
