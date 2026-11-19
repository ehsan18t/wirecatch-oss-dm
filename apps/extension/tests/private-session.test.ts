import { describe, expect, it } from 'vitest';
import { memoryArea } from '../src/lib/cache';
import { PRIVATE_SESSION_KEY, PrivateSession } from '../src/lib/private-session';
import { flush } from './fakes';

function session(options: { open: boolean; stored?: string }) {
  const area = memoryArea();
  if (options.stored !== undefined) void area.set({ [PRIVATE_SESSION_KEY]: options.stored });
  const ended: string[] = [];
  const windows = { open: options.open };
  let next = 0;
  const followed = new PrivateSession({
    area,
    anyPrivateWindow: () => Promise.resolve(windows.open),
    newId: () => `id-${String((next += 1))}`,
    ended: (id) => ended.push(id),
  });
  return { followed, windows, ended, area };
}

describe('the private session', () => {
  it('starts with the first private window and ends with the last (D279)', async () => {
    const s = session({ open: false });
    expect(await s.followed.current()).toBeNull();
    s.windows.open = true;
    expect(await s.followed.current()).toBe('id-1');
    expect(await s.followed.current()).toBe('id-1');
    s.windows.open = false;
    await s.followed.windowsChanged();
    expect(s.ended).toEqual(['id-1']);
    s.windows.open = true;
    expect(await s.followed.current()).toBe('id-2');
  });

  it('keeps its id across a reload of the extension while private windows stay open', async () => {
    const s = session({ open: true, stored: 'kept' });
    expect(await s.followed.current()).toBe('kept');
    expect(s.ended).toEqual([]);
  });

  it('ends a session found at start with no private window open', async () => {
    const s = session({ open: false, stored: 'stale' });
    await flush();
    expect(s.ended).toEqual(['stale']);
    expect(await s.followed.current()).toBeNull();
    expect((await s.area.get(PRIVATE_SESSION_KEY))[PRIVATE_SESSION_KEY]).toBeNull();
  });
});
