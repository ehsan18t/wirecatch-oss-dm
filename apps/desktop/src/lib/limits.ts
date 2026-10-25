// How many connections and downloads may run at once, and how fast. The core owns these limits (crates/protocol generates them); the fields take them as their range, the queue dialog checks what it saves against them, and a queue's own limits are said in short phrases.

import {
  MAX_CONCURRENT_DOWNLOADS,
  MAX_CONNECTIONS_PER_DOWNLOAD,
  ownValue,
  type QueueSettings,
} from '@wirecatch/protocol';
import { formatRate } from '@wirecatch/units';

/** Why `typed` cannot be a queue's downloads at the same time, or `null` when it can. */
export function queueConcurrentProblem(typed: number): string | null {
  const max = Math.round(typed);
  return Number.isFinite(max) && max >= 1 && max <= MAX_CONCURRENT_DOWNLOADS
    ? null
    : `Downloads at the same time must be between 1 and ${MAX_CONCURRENT_DOWNLOADS}.`;
}

/** Why `typed` cannot be a queue's connections per download, or `null` when it can. */
export function queueConnectionsProblem(typed: number): string | null {
  const connections = Math.round(typed);
  return Number.isFinite(connections) &&
    connections >= 1 &&
    connections <= MAX_CONNECTIONS_PER_DOWNLOAD
    ? null
    : `Connections per download must be between 1 and ${MAX_CONNECTIONS_PER_DOWNLOAD}.`;
}

/** A queue's own limits, one short phrase each, or that it follows the app's when it has none. */
export function queueLimitFacts(settings: QueueSettings): string[] {
  const concurrent = ownValue(settings.max_concurrent);
  const limit = ownValue(settings.speed_limit);
  const connections = ownValue(settings.connections);
  const facts = [
    concurrent === null ? '' : `${String(concurrent)} at once`,
    limit === null ? '' : `Up to ${formatRate(limit)}`,
    connections === null
      ? ''
      : `${String(connections)} ${connections === 1 ? 'connection' : 'connections'} each`,
  ].filter(Boolean);
  return facts.length > 0 ? facts : ["Follows the app's limits"];
}

/** What a queue's switch for downloads at the same time does, on and off, given the app's limit. */
export function concurrentLimitHelp(own: boolean, app: number | undefined): string {
  const appWide = app === undefined ? "the app's limit" : `the app's limit of ${String(app)}`;
  return own
    ? `The rest wait their turn; ${appWide} still applies.`
    : `Off, only ${appWide} applies.`;
}

/** What a queue's speed limit switch does, on and off, given the app's limit. */
export function speedLimitHelp(own: boolean, app: number | null): string {
  const appWide = app === null ? null : `the app's limit of ${formatRate(app)}`;
  if (own)
    return appWide === null
      ? 'Shared by everything running in this queue.'
      : `Shared by everything running in this queue; ${appWide} still applies.`;
  return appWide === null ? 'Off, it downloads at full speed.' : `Off, only ${appWide} applies.`;
}

/** What a queue's switch for its own connections per download does, on and off. */
export function ownConnectionsHelp(own: boolean, app: number): string {
  return own
    ? 'Each download in this queue opens up to this many.'
    : `Off, it uses the app's ${String(app)}.`;
}
