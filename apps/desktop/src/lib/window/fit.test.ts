import { describe, expect, it, vi } from 'vitest';
import { fitReporter, neededHeight } from './fit';

describe('a download window fits its content', () => {
  it('keeps everything around the body and makes the body as tall as its content', () => {
    // A frame 600 tall whose body takes 400 of it: the title and the buttons need the other 200.
    expect(neededHeight({ frame: 600, body: 400, content: 226.4, padding: 22 })).toBe(449);
    // Squeezed by a small screen, the body is shorter than its content; what it needs is the same.
    expect(neededHeight({ frame: 320, body: 120, content: 226.4, padding: 22 })).toBe(449);
  });

  it('reports nothing for a frame that is not laid out yet', () => {
    expect(neededHeight({ frame: 0, body: 0, content: 0, padding: 0 })).toBe(0);
    expect(neededHeight({ frame: Number.NaN, body: 0, content: 10, padding: 0 })).toBe(0);
  });

  it('resizes once per frame, only for a new height', () => {
    const fit = vi.fn();
    const runs: (() => void)[] = [];
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
