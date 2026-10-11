import { describe, expect, it, vi } from 'vitest';
import { fitReporter, neededHeight } from './fit';

describe('a download window fits its content', () => {
  it('adds up the title, the content, the body padding and the buttons', () => {
    expect(neededHeight({ head: 64.4, content: 300, padding: 22, foot: 61 })).toBe(448);
    expect(neededHeight({ head: 64, content: 300, padding: 22, foot: 0 })).toBe(386);
  });

  it('reports nothing for a frame that is not laid out yet', () => {
    expect(neededHeight({ head: 0, content: 0, padding: 0, foot: 0 })).toBe(0);
    expect(neededHeight({ head: Number.NaN, content: 10, padding: 0, foot: 0 })).toBe(0);
  });

  it('resizes once per frame, only for a new height', () => {
    const fit = vi.fn();
    const runs: Array<() => void> = [];
    const report = fitReporter(fit, (run) => runs.push(run));
    report(400);
    report(420);
    expect(runs).toHaveLength(1);
    runs.shift()?.();
    expect(fit.mock.calls).toEqual([[420]]);
    report(420);
    runs.shift()?.();
    expect(fit).toHaveBeenCalledTimes(1);
    report(0);
    runs.shift()?.();
    expect(fit).toHaveBeenCalledTimes(1);
    report(380);
    runs.shift()?.();
    expect(fit.mock.calls).toEqual([[420], [380]]);
  });
});
