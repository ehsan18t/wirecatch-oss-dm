// How many connections and downloads may run at once, and how fast. The core owns these limits (crates/protocol generates them); the fields take them as their range, the queue dialog checks what it saves against them, and a queue's own limits are said in short phrases.

import {
  MAX_CONCURRENT_DOWNLOADS,
  MAX_CONNECTIONS_PER_DOWNLOAD,
  ownValue,
  type Queue,
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

/** What a queue's downloads at the same time are: the app's (`own` `null`), or its own, which the app's limit still caps. */
export function concurrentLimitHelp(own: number | null, app: number | undefined): string {
  if (own === null)
    return app === undefined
      ? "Uses the app's limit."
      : `Uses the app's limit: ${String(app)} at once.`;
  const line = 'Up to this many of its downloads run at once, and the rest wait their turn.';
  if (app === undefined) return line;
  return own > app
    ? `${line} The app's limit of ${String(app)} is lower, so at most ${String(app)} run at once.`
    : `${line} The app's limit of ${String(app)} still applies to all downloads.`;
}

/** Where a queue's own downloads at the same time start: below the app's limit, so switching it on changes something. */
export function firstConcurrent(app: number): number {
  return Math.max(1, app - 1);
}

/** What a queue's speed limit is, its own or the app's; the app's limit still caps every download. */
export function speedLimitHelp(own: boolean, app: number | null): string {
  if (own)
    return app === null
      ? 'Shared by everything running in this queue.'
      : `Shared by everything running in this queue. The app's limit of ${formatRate(app)} still applies.`;
  return `Uses the app's limit: ${app === null ? 'none' : formatRate(app)}.`;
}

/** What a queue's connections per download are: its own replace the app's. */
export function ownConnectionsHelp(own: boolean, app: number): string {
  return own
    ? `Each download in this queue opens up to this many, instead of the app's ${String(app)}.`
    : `Uses the app's setting: ${String(app)} per download.`;
}

/** The queue settings that may follow an app setting or have their own value (D72.1). */
export type QueueLimit = 'max_concurrent' | 'speed_limit' | 'connections';

/** The queues with their own value for `limit`, in sidebar order. */
export function queuesWithOwn(queues: readonly Queue[], limit: QueueLimit): Queue[] {
  return queues.filter((q) => q.settings[limit].source === 'own');
}

/** What deleting a queue does with its `count` downloads, which move to the queue named `to`. */
export function queueDeletionNote(count: number, to: string): string {
  if (count === 0) return 'It holds no downloads.';
  return count === 1
    ? `Its download moves to ${to}.`
    : `Its ${String(count)} downloads move to ${to}.`;
}
