// Every action in the app, in one list. The command palette runs these; buttons and keyboard
// shortcuts call the same `AppState` methods, so there is one behavior per action. Each area
// keeps its commands in a table of its own; this puts them together in the palette's order.

import type { AppState } from '../state/app.svelte';
import { downloadCommands, helpCommands, viewCommands } from './library';
import { selectionCommands } from './selection';
import { addonCommands, settingsCommands } from './settings';
import type { Command } from './table';

export { focusSearch } from './library';
export { fuzzyScore, groupCommands, searchCommands } from './search';
export type { Command, CommandGroup } from './table';

export function buildCommands(app: AppState): Command[] {
  // Only the mock backend has sample-data actions; they live in the mock module, so none of
  // their text ships in a production build.
  const sample = app.ready ? app.backend.sampleData : undefined;
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
