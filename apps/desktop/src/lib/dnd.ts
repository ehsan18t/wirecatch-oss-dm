// Dragging downloads between queues and within a queue, and queues within the sidebar.

import { isDownloadId, type DownloadId, type QueueId } from '@wirecatch/protocol';

export const DRAG_TYPE = 'application/x-wirecatch-downloads';
const QUEUE_DRAG_TYPE = 'application/x-wirecatch-queue';

/** The queue being dragged: a drag's data cannot be read until it is dropped, and the sidebar shows where it would land before then. */
let draggedQueue: QueueId | null = null;

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

export function startQueueDrag(e: DragEvent, queue: QueueId, name: string): void {
  if (!e.dataTransfer) return;
  draggedQueue = queue;
  e.dataTransfer.effectAllowed = 'move';
  e.dataTransfer.setData(QUEUE_DRAG_TYPE, String(queue));
  e.dataTransfer.setData('text/plain', name);
}

export function endQueueDrag(): void {
  draggedQueue = null;
}

/** The queue a drag carries, or `null` for any other drag. */
export function getDraggedQueue(e: DragEvent): QueueId | null {
  return (e.dataTransfer?.types.includes(QUEUE_DRAG_TYPE) ?? false) ? draggedQueue : null;
}
