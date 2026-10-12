import { describe, expect, it, vi } from 'vitest';
import { commandsFrom, type CommandSpec } from './table';

interface Situation {
  name: string;
  open: boolean;
}

describe('commandsFrom', () => {
  const run = vi.fn();
  const table: readonly CommandSpec<Situation>[] = [
    { id: 'fixed', title: 'Fixed', run },
    {
      id: (s) => `named.${s.name}`,
      title: (s) => `About ${s.name}`,
      keywords: (s) => `more ${s.name}`,
      key: 'add',
      run,
    },
    { id: 'only-open', title: 'Only when open', when: (s) => s.open, run },
  ];

  it('keeps the rows that apply, in table order, with their text worked out', () => {
    const commands = commandsFrom('Downloads', table, { name: 'x', open: false });
    expect(commands.map((c) => [c.id, c.title, c.group, c.keywords, c.shortcut])).toEqual([
      ['fixed', 'Fixed', 'Downloads', undefined, undefined],
      ['named.x', 'About x', 'Downloads', 'more x', 'Ctrl+N'],
    ]);
    expect(commandsFrom('Downloads', table, { name: 'x', open: true }).at(-1)?.id).toBe(
      'only-open',
    );
  });

  it('runs a row against the situation it was built for', () => {
    const situation = { name: 'y', open: true };
    commandsFrom('Help', table, situation)[1]?.run();
    expect(run).toHaveBeenCalledWith(situation);
  });
});
