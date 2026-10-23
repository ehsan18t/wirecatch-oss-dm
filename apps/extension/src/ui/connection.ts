// How the extension's pages put the connection to the app into words.
import type { Tone } from '@wirecatch/ui';
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

/** Stands in for the settings while the app has not sent them: they live in the app (D61). */
export function waitingNote(status: ConnectionStatus): string {
  switch (status.state) {
    case 'connecting':
      return 'Connecting to Wirecatch. Your settings show here once it is connected.';
    case 'connected':
      return 'Getting your settings from Wirecatch.';
    case 'unavailable':
      return status.problem.kind === 'app_closed'
        ? 'Your settings are kept in Wirecatch, and show here once it is running.'
        : 'Your settings are kept in Wirecatch, and show here once it is connected.';
  }
}

/** Why a control that needs the app cannot be used now; null while connected. */
export function offlineReason(connected: boolean): string | null {
  return connected ? null : 'Wirecatch is not connected.';
}
