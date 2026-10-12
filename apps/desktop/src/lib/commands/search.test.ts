import { describe, expect, it } from 'vitest';
import { groupCommands } from './search';
import type { Command } from './table';

const command = (id: string, group: Command['group']): Command => ({
  id,
  title: id,
  group,
  run: () => undefined,
});

describe('groupCommands', () => {
  it('keeps the groups in the order they first appear and the commands in theirs', () => {
    const list = [
      command('a', 'Downloads'),
      command('b', 'Show'),
      command('c', 'Downloads'),
      command('d', 'Help'),
    ];
    expect(groupCommands(list).map((g) => [g.name, g.items.map((c) => c.id)])).toEqual([
      ['Downloads', ['a', 'c']],
      ['Show', ['b']],
      ['Help', ['d']],
    ]);
  });
});
