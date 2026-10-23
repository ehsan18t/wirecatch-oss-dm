// How many connections and downloads may run at once. The core owns these limits (crates/protocol generates them); the fields take them as their range, and the queue dialog checks what it saves against them.

import { MAX_CONCURRENT_DOWNLOADS } from '@wirecatch/protocol';

/** Why `typed` cannot be a queue's downloads at the same time, or `null` when it can. */
export function queueConcurrentProblem(typed: number): string | null {
  const max = Math.round(typed);
  return Number.isFinite(max) && max >= 1 && max <= MAX_CONCURRENT_DOWNLOADS
    ? null
    : `Downloads at the same time must be between 1 and ${MAX_CONCURRENT_DOWNLOADS}.`;
}
