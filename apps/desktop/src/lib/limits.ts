// How many connections and downloads may run at once, and how fast. The core owns these limits (crates/protocol generates them); the fields take them as their range, the queue dialog checks what it saves against them, and a queue's own limits are said in short phrases.

import {
  GLOBAL,
  MAX_CONCURRENT_DOWNLOADS,
  MAX_CONNECTIONS_PER_DOWNLOAD,
  ownSetting,
  ownValue,
  type Queue,
  type QueueSettings,
} from '@wirecatch/protocol';
import { formatRate, toBytes, type SizeUnit } from '@wirecatch/units';

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
  if (facts.length === 0) return ['Uses global limits'];
  const follows = [concurrent, limit, connections].some((own) => own === null);
  return follows ? [...facts, 'Other limits are global'] : facts;
}

/** A warning when a queue's own downloads at the same time exceed the global limit, which still caps them. */
export function concurrentCapWarning(own: number, app: number | undefined): string | null {
  if (app === undefined || own <= app) return null;
  return `The global limit of ${String(app)} is lower, so at most ${String(app)} run at once.`;
}

/** Where a queue's own downloads at the same time start: below the app's limit, so switching it on changes something. */
export function firstConcurrent(app: number): number {
  return Math.max(1, app - 1);
}

/** The global value a queue limit follows while it has none of its own; `null` before the settings load. */
export function globalLimitText(limit: QueueLimit, app: number | null | undefined): string | null {
  if (app === undefined) return null;
  if (limit === 'speed_limit') return `Global: ${app === null ? 'no limit' : formatRate(app)}`;
  return app === null ? null : `Global: ${String(app)}`;
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

/** The speed limit a queue editor describes (`on`, and `value` in `unit`), or why it is not one. */
export function readQueueLimit(
  on: boolean,
  value: number | null,
  unit: SizeUnit,
): { limit: QueueSettings['speed_limit'] } | string {
  if (!on) return { limit: GLOBAL };
  const limit = value === null ? null : toBytes(value, unit);
  return limit === null || limit <= 0
    ? 'Enter a speed limit above zero, or turn the limit off.'
    : { limit: ownSetting(limit) };
}
