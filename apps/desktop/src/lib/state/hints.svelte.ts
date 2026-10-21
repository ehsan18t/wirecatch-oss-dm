// One-time contextual hints. Each shows once, at the moment it is useful, and never again
// after it is dismissed. Only this display preference lives in the webview's storage.

import { SvelteSet } from 'svelte/reactivity';
import { shortcutLabel } from '../keymap';
import { readStored, writeStored } from './page-storage';

export type HintId = 'palette' | 'drag-to-queue' | 'reorder' | 'paste';

export const HINT_TEXT: Record<HintId, string> = {
  paste: `Copy a link anywhere, then press ${shortcutLabel('paste')} in this window to download it.`,
  palette: `Press ${shortcutLabel('palette')} to find any action or setting by typing a few letters.`,
  'drag-to-queue':
    'Drag downloads onto a queue in the sidebar to move them. Each queue has its own limits and schedule.',
  reorder: `Drag a download by its number to change its place in its queue. ${shortcutLabel('nudge-up')} and ${shortcutLabel('nudge-down')} work too.`,
};

const KEY = 'wirecatch.hints.dismissed';

function isHintId(value: unknown): value is HintId {
  return typeof value === 'string' && Object.hasOwn(HINT_TEXT, value);
}

function load(): HintId[] {
  try {
    const raw = readStored(KEY);
    const parsed: unknown = raw === null ? [] : JSON.parse(raw);
    return Array.isArray(parsed) ? parsed.filter(isHintId) : [];
  } catch {
    return [];
  }
}

export class Hints {
  readonly dismissed = new SvelteSet<HintId>(load());

  isOpen(id: HintId): boolean {
    return !this.dismissed.has(id);
  }

  dismiss(id: HintId): void {
    this.dismissed.add(id);
    // Where storage is unavailable, the hint just shows again next time.
    writeStored(KEY, JSON.stringify([...this.dismissed]));
  }
}
