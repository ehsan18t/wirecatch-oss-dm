// How a command is described: a table row with its text, an optional `when` that says if it
// applies right now, and what it runs. Each area of the app keeps its own table; `commandsFrom`
// turns one into the palette's commands for the situation at hand.

import { shortcutLabel, type KeyCommand } from '../keymap';

export type CommandGroup =
  'Selected downloads' | 'Downloads' | 'Show' | 'Queues' | 'Settings' | 'Help' | 'Sample data';

export interface Command {
  id: string;
  title: string;
  group: CommandGroup;
  keywords?: string;
  /** The key that does the same, as people read it. */
  shortcut?: string;
  run: () => unknown;
}

/** Text fixed in the table, or worked out from the situation. */
type Text<C> = string | ((c: C) => string);

/** One row of a command table, read against a situation `C`. */
export interface CommandSpec<C> {
  id: Text<C>;
  title: Text<C>;
  /** More words the palette finds the command by. */
  keywords?: Text<C>;
  /** The keyboard shortcut that does the same, from the keymap. */
  key?: KeyCommand;
  /** Left out, the command always applies. */
  when?: (c: C) => boolean;
  run: (c: C) => unknown;
}

function read<C>(text: Text<C>, c: C): string {
  return typeof text === 'string' ? text : text(c);
}

/** The commands of a table that apply to situation `c`, in table order. */
export function commandsFrom<C>(
  group: CommandGroup,
  specs: readonly CommandSpec<C>[],
  c: C,
): Command[] {
  return specs
    .filter((s) => s.when?.(c) ?? true)
    .map((s) => ({
      id: read(s.id, c),
      title: read(s.title, c),
      group,
      keywords: s.keywords === undefined ? undefined : read(s.keywords, c),
      shortcut: s.key === undefined ? undefined : shortcutLabel(s.key),
      run: () => s.run(c),
    }));
}
