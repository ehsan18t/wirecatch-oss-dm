// Every action in the app, in one list. The command palette runs these, the window's keys run
// the command their key belongs to (`runKey`), and buttons call the same `AppState` methods, so
// there is one behavior per action. Each area
// keeps its commands in a table of its own; this puts them together in the palette's order.

import type { KeyCommand } from '../keymap';
import type { AppState } from '../state/app.svelte';
import { downloadCommands, helpCommands, viewCommands } from './library';
import { selectionCommands } from './selection';
import { addonCommands, settingsCommands } from './settings';
import type { Command } from './table';

export { fuzzyScore, groupCommands, searchCommands } from './search';
export type { Command, CommandGroup } from './table';

export function buildCommands(app: AppState): Command[] {
  // Only the mock backend has sample-data actions; they live in the mock module, so none of
  // their text ships in a production build.
  const sample = app.sample;
  return [
    ...selectionCommands(app),
    ...downloadCommands(app),
    ...viewCommands(app),
    ...settingsCommands(app),
    ...helpCommands(app),
    ...addonCommands(app),
    ...(sample?.commands ?? []).map((c): Command => ({ ...c, group: 'Sample data' })),
  ];
}

/**
 * Runs the command that key `key` stands for right now, as the palette would run it. False when
 * no command for it applies (Escape with nothing selected, say).
 */
export function runKey(app: AppState, key: KeyCommand): boolean {
  const command = buildCommands(app).find((c) => c.key === key);
  if (!command) return false;
  void command.run();
  return true;
}
