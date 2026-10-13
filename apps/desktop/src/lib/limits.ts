// How many connections and downloads may run at once. The core owns these limits (crates/protocol generates them); this only applies them to what a person typed, so the settings screen and the queue dialog accept exactly what the core accepts.

import { MAX_CONCURRENT_DOWNLOADS, MAX_CONNECTIONS_PER_DOWNLOAD } from '@wirecatch/protocol';

/** The number in `typed` as a whole number from 1 to `max`, or `null` when it holds no number. */
function wholeUpTo(typed: string, max: number): number | null {
  const value = typed.trim() === '' ? Number.NaN : Number(typed);
  return Number.isFinite(value) ? Math.min(max, Math.max(1, Math.round(value))) : null;
}

/** Connections per download from what a person typed; `null` when it is not a number. */
export function connectionsPerDownload(typed: string): number | null {
  return wholeUpTo(typed, MAX_CONNECTIONS_PER_DOWNLOAD);
}

/** Downloads running at once across all queues from what a person typed; `null` when it is not a number. */
export function concurrentDownloads(typed: string): number | null {
  return wholeUpTo(typed, MAX_CONCURRENT_DOWNLOADS);
}

/** Why `typed` cannot be a queue's downloads at the same time, or `null` when it can. */
export function queueConcurrentProblem(typed: number): string | null {
  const max = Math.round(typed);
  return Number.isFinite(max) && max >= 1 && max <= MAX_CONCURRENT_DOWNLOADS
    ? null
    : `Downloads at the same time must be between 1 and ${MAX_CONCURRENT_DOWNLOADS}.`;
}
