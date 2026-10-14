import { describe, expect, it, vi } from 'vitest';
import { MockBackend } from '$lib/backend/mock';
import { Connection } from './connection';

describe('the connection to the backend', () => {
  it('loads one backend, however many ask for it while it loads', async () => {
    const made: MockBackend[] = [];
    const load = vi.fn(async () => {
      await Promise.resolve();
      const backend = new MockBackend({ autoTick: false });
      made.push(backend);
      return backend;
    });
    const connection = new Connection(load);
    // The title bar asks for the window while the page is still starting.
    const [first, second] = await Promise.all([connection.load(), connection.load()]);
    expect(load).toHaveBeenCalledTimes(1);
    expect(first).toBe(second);
    expect(connection.loaded).toBe(first);
    for (const backend of made) backend.dispose();
  });

  it('tries again after a load that failed', async () => {
    const backend = new MockBackend({ autoTick: false });
    const load = vi
      .fn<() => Promise<MockBackend>>()
      .mockRejectedValueOnce(new Error('The engine did not start.'))
      .mockResolvedValue(backend);
    const connection = new Connection(load);
    await expect(connection.load()).rejects.toThrow('The engine did not start.');
    await expect(connection.load()).resolves.toBe(backend);
    backend.dispose();
  });
});
