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
  if (facts.length === 0) return ["Follows the app's limits"];
  const follows = [concurrent, limit, connections].some((own) => own === null);
  return follows ? [...facts, 'Other limits follow the app'] : facts;
}

/** What a queue's downloads at the same time are, its own or the app's; the app's limit still caps every download. */
export function concurrentLimitHelp(own: boolean, app: number | undefined): string {
  if (app === undefined)
    return own ? 'Others in this queue wait their turn.' : "Uses the app's limit.";
  return own
    ? `Others in this queue wait their turn. The app's limit of ${String(app)} still applies to all downloads.`
    : `Uses the app's limit: ${String(app)} at once.`;
}

/** What a queue's speed limit is, its own or the app's; the app's limit still caps every download. */
export function speedLimitHelp(own: boolean, app: number | null): string {
  if (own)
    return app === null
      ? 'Shared by everything running in this queue.'
      : `Shared by everything running in this queue. The app's limit of ${formatRate(app)} still applies.`;
  return app === null
    ? "Uses the app's speed limit, which is off."
    : `Uses the app's speed limit: ${formatRate(app)}.`;
}

/** What a queue's connections per download are: its own replace the app's. */
export function ownConnectionsHelp(own: boolean, app: number): string {
  return own
    ? `Each download in this queue opens up to this many, instead of the app's ${String(app)}.`
    : `Uses the app's ${String(app)}.`;
}
