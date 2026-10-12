// The order of downloads inside a queue: where dragged rows land, and where Alt+Up and Alt+Down
// move the selection. Pure, so the state classes only send the result to the core.

import type { DownloadId } from '@wirecatch/protocol';

/**
 * `current` with `ids` moved to sit just before `before`, or at the end when it is `null` (or
 * not in the queue). Only ids in `current` take part. `null` when the order would not change.
 */
export function placed(
  current: readonly DownloadId[],
  ids: readonly DownloadId[],
  before: DownloadId | null,
): DownloadId[] | null {
  const moving = current.filter((id) => ids.includes(id));
  if (moving.length === 0) return null;
  const rest = current.filter((id) => !ids.includes(id));
  const found = before === null ? -1 : rest.indexOf(before);
  const at = found < 0 ? rest.length : found;
  const next = [...rest.slice(0, at), ...moving, ...rest.slice(at)];
  return next.every((id, i) => id === current[i]) ? null : next;
}

/** Where a nudge puts the selected downloads: before `before`, or at the end when `null`. */
export interface Nudge {
  chosen: DownloadId[];
  before: DownloadId | null;
}

/**
 * Moving the selected downloads of a queue one place up (`-1`) or down (`1`). `null` when they
 * are already at that end, or none of the queue is selected.
 */
export function nudged(
  current: readonly DownloadId[],
  isSelected: (id: DownloadId) => boolean,
  delta: -1 | 1,
): Nudge | null {
  const chosen = current.filter(isSelected);
  const firstChosen = chosen.at(0);
  const lastChosen = chosen.at(-1);
  if (firstChosen === undefined || lastChosen === undefined) return null;
  if (delta < 0) {
    const first = current.indexOf(firstChosen);
    if (first === 0) return null;
    // The row just above the first selected one is never selected itself.
    return { chosen, before: current[first - 1] ?? null };
  }
  const last = current.indexOf(lastChosen);
  if (last === current.length - 1) return null;
  const rest = current.filter((id) => !isSelected(id));
  const after = current[last + 1];
  const i = after === undefined ? -1 : rest.indexOf(after);
  return { chosen, before: i < 0 ? null : (rest[i + 1] ?? null) };
}
