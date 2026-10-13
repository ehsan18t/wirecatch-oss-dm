// Running an action for a window: a failure is said once, in words, and a window that shows only
// the latest failure takes the last one away as the next action starts.

import { downloadId } from '@wirecatch/protocol';
import { describe, expect, it, vi } from 'vitest';
import { Runner } from './runner';

describe('Runner', () => {
  it('says why an action failed and gives nothing back', async () => {
    const fail = vi.fn();
    const runner = new Runner(fail);
    const result = await runner.run(() => Promise.reject<number>(new Error('No disk.')));
    expect(result).toBeUndefined();
    expect(fail).toHaveBeenCalledExactlyOnceWith('No disk.');
    expect(await runner.succeeds(() => Promise.resolve())).toBe(true);
  });

  it('clears the last failure as each action starts', async () => {
    const shown: (string | null)[] = [];
    const runner = new Runner(
      (text) => shown.push(text),
      () => shown.push(null),
    );
    await runner.run(() => Promise.reject(new Error('Offline.')));
    await runner.succeeds(() => Promise.resolve());
    expect(shown).toEqual([null, 'Offline.', null]);
  });

  it('clears the last failure as a bulk action starts', async () => {
    const clear = vi.fn();
    const runner = new Runner(() => undefined, clear);
    await runner.bulk([downloadId(1), downloadId(2)], 'pause', () => Promise.resolve());
    expect(clear).toHaveBeenCalledOnce();
  });

  it('leaves a failure the caller recovers from unsaid', async () => {
    const fail = vi.fn();
    const runner = new Runner(fail);
    const expected = new Error('Install the add-on first.');
    const recover = vi.fn((err: unknown) => err === expected);
    expect(await runner.run(() => Promise.reject<number>(expected), recover)).toBeUndefined();
    expect(recover).toHaveBeenCalledWith(expected);
    expect(fail).not.toHaveBeenCalled();
    await runner.run(() => Promise.reject<number>(new Error('Offline.')), recover);
    expect(fail).toHaveBeenCalledExactlyOnceWith('Offline.');
  });
});
