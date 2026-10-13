// Dragging downloads between queues and within a queue.

import { isDownloadId, type DownloadId } from '@wirecatch/protocol';

export const DRAG_TYPE = 'application/x-wirecatch-downloads';

export function setDragIds(e: DragEvent, ids: readonly DownloadId[]): void {
  if (!e.dataTransfer) return;
  e.dataTransfer.effectAllowed = 'move';
  e.dataTransfer.setData(DRAG_TYPE, JSON.stringify(ids));
  e.dataTransfer.setData('text/plain', `${ids.length} download${ids.length === 1 ? '' : 's'}`);
}

export function isDownloadDrag(e: DragEvent): boolean {
  return e.dataTransfer?.types.includes(DRAG_TYPE) ?? false;
}

export function getDragIds(e: DragEvent): DownloadId[] {
  const raw = e.dataTransfer?.getData(DRAG_TYPE);
  if (!raw) return [];
  try {
    const parsed: unknown = JSON.parse(raw);
    return Array.isArray(parsed) ? parsed.filter(isDownloadId) : [];
  } catch {
    return [];
  }
}
