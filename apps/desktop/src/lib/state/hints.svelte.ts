// One-time contextual hints. Each shows once, at the moment it is useful, and never again
// after it is dismissed. Only this display preference lives in the webview's storage.

import { shortcutLabel } from '../keymap';

export type HintId = 'palette' | 'drag-to-queue' | 'reorder' | 'paste';

export const HINT_TEXT: Record<HintId, string> = {
  paste: `Copy a link anywhere, then press ${shortcutLabel('paste')} in this window to download it.`,
  palette: `Press ${shortcutLabel('palette')} to find any action or setting by typing a few letters.`,
  'drag-to-queue':
    'Drag downloads onto a queue in the sidebar to move them. Each queue has its own limits and schedule.',
  reorder: `Drag downloads up or down to change the order this queue runs them in. ${shortcutLabel('nudge-up')} and ${shortcutLabel('nudge-down')} work too.`,
};

/* eslint-disable svelte/prefer-svelte-reactivity -- the set is replaced whole on each change. */

const KEY = 'wirecatch.hints.dismissed';

/** The webview's storage, or null where there is none (tests, and pages without a window). */
function storage(): Storage | null {
  return 'localStorage' in globalThis ? globalThis.localStorage : null;
}

function isHintId(value: unknown): value is HintId {
  return typeof value === 'string' && Object.hasOwn(HINT_TEXT, value);
}

function load(): Set<HintId> {
  try {
    const raw = storage()?.getItem(KEY) ?? null;
    const parsed: unknown = raw === null ? [] : JSON.parse(raw);
    return new Set(Array.isArray(parsed) ? parsed.filter(isHintId) : []);
  } catch {
    return new Set();
  }
}

export class Hints {
  dismissed = $state(load());

  isOpen(id: HintId): boolean {
    return !this.dismissed.has(id);
  }

  dismiss(id: HintId): void {
    const next = new Set(this.dismissed);
    next.add(id);
    this.dismissed = next;
    try {
      storage()?.setItem(KEY, JSON.stringify([...next]));
    } catch {
      // Storage can be unavailable; the hint just shows again next time.
    }
  }
}
