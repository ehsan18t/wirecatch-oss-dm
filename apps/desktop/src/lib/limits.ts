// How many connections and downloads may run at once. The core owns these limits and their
// defaults (crates/protocol generates them); this only applies them to what a person typed, so
// the settings screen and the queue dialog accept exactly what the core accepts.

import {
  DEFAULT_CONNECTIONS_PER_DOWNLOAD,
  DEFAULT_MAX_CONCURRENT_DOWNLOADS,
  MAX_CONCURRENT_DOWNLOADS,
  MAX_CONNECTIONS_PER_DOWNLOAD,
} from '@wirecatch/protocol';

/** `typed` as a whole number from 1 to `max`, or `fallback` when it is not a number. */
function wholeUpTo(typed: number, max: number, fallback: number): number {
  return Number.isFinite(typed) ? Math.min(max, Math.max(1, Math.round(typed))) : fallback;
}

/** Connections per download from a typed value; the default when it is not a number. */
export function connectionsPerDownload(typed: number): number {
  return wholeUpTo(typed, MAX_CONNECTIONS_PER_DOWNLOAD, DEFAULT_CONNECTIONS_PER_DOWNLOAD);
}

/** Downloads running at once across all queues from a typed value; the default when it is not a number. */
export function concurrentDownloads(typed: number): number {
  return wholeUpTo(typed, MAX_CONCURRENT_DOWNLOADS, DEFAULT_MAX_CONCURRENT_DOWNLOADS);
}

/** Why `typed` cannot be a queue's downloads at the same time, or `null` when it can. */
export function queueConcurrentProblem(typed: number): string | null {
  const max = Math.round(typed);
  return Number.isFinite(max) && max >= 1 && max <= MAX_CONCURRENT_DOWNLOADS
    ? null
    : `Downloads at the same time must be between 1 and ${MAX_CONCURRENT_DOWNLOADS}.`;
}
