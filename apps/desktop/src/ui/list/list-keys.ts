// What each key does while the download list has the keyboard. The keys come from the keymap, so the shortcuts dialog lists the same ones.

import type { ListKey } from '$lib/keymap';
import type { AppState } from '$lib/state/app.svelte';

/** What the keys need from the list: how far a page moves and how many rows it shows. */
export interface ListKeysHost {
  app: AppState;
  pageRows: () => number;
  count: () => number;
}

/** Enter: open a finished file, or run the fix a download that needs attention offers. */
function openOrFix(app: AppState): void {
  const only = app.selection.rows.length === 1 ? app.selection.rows[0] : undefined;
  if (only?.status === 'completed') void app.actions.openFile(only.id);
  else if (only?.status === 'failed' && only.error?.fix)
    void app.actions.fix(only.id, only.error.fix);
}

/** Space: pause what runs in the selection, or resume it, and tell a screen reader, whose place may be on another row, what changed. A failure says itself; nothing changed says nothing. */
async function pauseOrResume(app: AppState): Promise<void> {
  const rows = app.selection.rows;
  if (rows.length === 0) return;
  const { verb, done } = await app.actions.togglePause();
  if (verb === null || done === 0) return;
  const only = rows.length === 1 ? rows[0]?.file_name : undefined;
  const what = only ?? (done === 1 ? '1 download' : `${String(done)} downloads`);
  app.toasts.toaster.announce(`${verb === 'pause' ? 'Paused' : 'Resumed'} ${what}`);
}

/** The keyboard row is acted on when nothing is selected. */
function selectActive(app: AppState): void {
  if (app.selection.size === 0 && app.selection.active !== null)
    app.selection.only(app.selection.active);
}

/** An arrow: with nothing selected, it selects the row the list points at (where it took the keyboard); otherwise it moves. */
function step(app: AppState, delta: 1 | -1): void {
  const selection = app.selection;
  if (selection.size === 0 && selection.active !== null) selection.only(selection.active);
  else selection.move(delta, false);
}

/**
 * Each list key's action. The Menu key and Shift+F10 are the context menu's own, which opens beside the keyboard row, so the list leaves them alone (`null`).
 */
export function listKeys({
  app,
  pageRows,
  count,
}: ListKeysHost): Record<ListKey, ((e: KeyboardEvent) => void) | null> {
  const selection = app.selection;
  return {
    up: () => step(app, -1),
    down: () => step(app, 1),
    'extend-up': () => selection.move(-1, true),
    'extend-down': () => selection.move(1, true),
    'page-up': (e) => selection.move(-pageRows(), e.shiftKey),
    'page-down': (e) => selection.move(pageRows(), e.shiftKey),
    first: (e) => selection.move(-count(), e.shiftKey),
    last: (e) => selection.move(count(), e.shiftKey),
    'select-all': () => selection.all(),
    'pause-resume': () => {
      selectActive(app);
      void pauseOrResume(app);
    },
    open: () => {
      selectActive(app);
      openOrFix(app);
    },
    details: () => {
      selectActive(app);
      if (selection.size > 0) app.openDetails();
    },
    remove: () => {
      if (selection.size > 0) app.openDialog({ kind: 'remove', ids: selection.list });
    },
    'nudge-up': () => void app.actions.nudge(-1),
    'nudge-down': () => void app.actions.nudge(1),
    menu: null,
  };
}
