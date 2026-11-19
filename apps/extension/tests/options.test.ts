import { MAX_OPTIONS_PER_GROUP } from '@wirecatch/protocol';
import { DEFAULT_OPTIONS } from '@wirecatch/protocol/testing/options';
import { describe, expect, it } from 'vitest';
import { parseOption, parseOptions } from '../src/lib/options';

const toggle = (id: string) => ({ id, kind: 'toggle', label: id, value: true });

describe('the option parser', () => {
  it('reads every option a fresh install sends', () => {
    const groups = parseOptions(DEFAULT_OPTIONS);
    const sent = DEFAULT_OPTIONS.groups.reduce((n, group) => n + group.options.length, 0);
    const read = groups.reduce((n, group) => n + group.options.length, 0);
    expect(read).toBe(sent);
    expect(groups.flatMap((group) => group.options).some((o) => o.kind === 'unknown')).toBe(false);
  });

  it('shows a kind it does not know as one it opens in the app, an inherited name included', () => {
    for (const kind of ['slider', 'constructor', 'toString', '__proto__']) {
      expect(parseOption({ id: 'x', kind, label: 'X' })).toEqual({
        kind: 'unknown',
        id: 'x',
        label: 'X',
      });
    }
  });

  it('skips an option it cannot read and keeps its neighbours', () => {
    const [group] = parseOptions({
      revision: { instance: 'a', counter: 1 },
      groups: [
        {
          id: 'g',
          title: null,
          summary: null,
          folded: false,
          options: [
            toggle('first'),
            { id: 'broken', kind: 'toggle', label: 'Broken', value: 'yes' },
            null,
            'text',
            toggle('last'),
          ],
        },
      ],
    });
    expect(group?.options.map((o) => o.id)).toEqual(['first', 'last']);
  });

  it('keeps no more options than the limit', () => {
    const options = Array.from({ length: MAX_OPTIONS_PER_GROUP + 5 }, (_, i) => toggle(`t${i}`));
    const [group] = parseOptions({
      revision: { instance: 'a', counter: 1 },
      groups: [{ id: 'g', title: null, summary: null, folded: false, options }],
    });
    expect(group?.options).toHaveLength(MAX_OPTIONS_PER_GROUP);
  });
});
