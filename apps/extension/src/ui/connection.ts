// How the extension's pages put the connection to the app into words.
import type { Tone } from '@wirecatch/ui';
import type { Snapshot } from '../lib/messages';
import type { ConnectionStatus } from '../lib/native/status';

/** The connection at a glance: a word or two and the tone of its badge. */
export function connectionBadge(status: ConnectionStatus): { label: string; tone: Tone } {
  switch (status.state) {
    case 'connected':
      return { label: 'Connected', tone: 'success' };
    case 'connecting':
      return { label: 'Connecting', tone: 'neutral' };
    case 'unavailable':
      if (status.problem.kind === 'app_closed') return { label: 'Closed', tone: 'neutral' };
      return status.retryAt === null
        ? { label: 'Connecting', tone: 'neutral' }
        : { label: 'Not connected', tone: 'warning' };
  }
}

/** Says that the settings shown are the last saved copy, and when it was saved; null while they are the app's own. */
export function cachedNote(snapshot: Snapshot | null): string | null {
  if (!snapshot?.settingsFromCache) return null;
  const when =
    snapshot.cachedAt === null ? '' : ` from ${new Date(snapshot.cachedAt).toLocaleString()}`;
  return `Showing your last saved settings${when}, read-only until Wirecatch is connected.`;
}

/** Stands in for the settings before the extension has ever heard from the app. */
export function waitingNote(status: ConnectionStatus): string {
  return status.state === 'connecting'
    ? 'Connecting to Wirecatch. Your settings show here once it is connected.'
    : 'Your settings show here once Wirecatch is connected.';
}

/** Why a control that needs the app cannot be used now; null while connected. */
export function offlineReason(connected: boolean): string | null {
  return connected ? null : 'Wirecatch is not connected.';
}
