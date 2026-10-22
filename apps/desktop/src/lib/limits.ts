// How many connections and downloads may run at once. The core owns these limits (crates/protocol generates them); the fields take them as their range, and the queue dialog checks what it saves against them.

import { MAX_CONCURRENT_DOWNLOADS } from '@wirecatch/protocol';

/** Where a speed limit field starts and how far each step goes, in each unit it is typed in: the queue dialog's and the total limit's. */
export const SPEED_FIELD = {
  KB: { min: 1, step: 50 },
  MB: { min: 0.1, step: 0.5 },
} as const;

/** Why `typed` cannot be a queue's downloads at the same time, or `null` when it can. */
export function queueConcurrentProblem(typed: number): string | null {
  const max = Math.round(typed);
  return Number.isFinite(max) && max >= 1 && max <= MAX_CONCURRENT_DOWNLOADS
    ? null
    : `Downloads at the same time must be between 1 and ${MAX_CONCURRENT_DOWNLOADS}.`;
}
