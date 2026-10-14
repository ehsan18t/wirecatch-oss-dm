import { describe, expect, it, vi } from 'vitest';
import { writeToConsole } from '../src/lib/diag/console';
import { diagnostics, Diagnostics, fallback, log, type DiagEntry } from '../src/lib/diag/log';
import {
  DIAG_KEY_PREFIX,
  forgetPrivate,
  logUncaught,
  persistDiagnostics,
  readDiagnostics,
  type DiagStore,
  type ErrorEvents,
} from '../src/lib/diag/persist';
import { flush } from './fakes';

describe('Diagnostics', () => {
  it('keeps the newest entries up to its capacity, oldest first', () => {
    let now = 0;
    const diag = new Diagnostics({ capacity: 3, now: () => ++now });
    for (const n of [1, 2, 3, 4, 5]) diag.log('warn', 'hub', `entry ${String(n)}`);
    expect(diag.entries().map((e) => e.message)).toEqual(['entry 3', 'entry 4', 'entry 5']);
    expect(diag.entries()[0]?.at).toBe(3);
  });

  it('redacts the message and the error', () => {
    const diag = new Diagnostics();
    diag.log('error', 'capture', 'Lost https://x.example/a', new Error('Cookie: a=b'));
    expect(diag.entries()).toEqual([
      expect.objectContaining({
        level: 'error',
        scope: 'capture',
        message: 'Lost https://x.example/…',
        detail: 'Error: Cookie: [redacted]',
      }),
    ]);
  });

  it('keeps debug entries only when asked to, and echoes every entry', () => {
    const echoed: DiagEntry[] = [];
    const diag = new Diagnostics({ echo: (e) => echoed.push(e) });
    diag.log('debug', 'media', 'noise');
    expect(diag.entries()).toEqual([]);
    expect(echoed).toHaveLength(1);
    const dev = new Diagnostics({ keepDebug: true });
    dev.log('debug', 'media', 'noise');
    expect(dev.entries()).toHaveLength(1);
  });

  it('puts restored entries first, within capacity', () => {
    const diag = new Diagnostics({ capacity: 2, now: () => 9 });
    diag.log('warn', 'hub', 'new');
    const old = saved(1, 'hub', 'old');
    diag.restore([old, old]);
    expect(diag.entries().map((e) => e.message)).toEqual(['old', 'new']);
  });

  it('counts a repeat instead of adding it, so one failing path cannot flush the buffer', () => {
    let now = 0;
    const diag = new Diagnostics({ capacity: 5, now: () => ++now });
    diag.log('error', 'capture', 'the cause');
    for (let i = 0; i < 50; i += 1) diag.log('error', 'media', 'hot path', new Error('same'));
    diag.log('warn', 'media', 'hot path', new Error('other'));
    expect(diag.entries()).toEqual([
      expect.objectContaining({ message: 'the cause', repeated: 0 }),
      expect.objectContaining({ message: 'hot path', detail: 'Error: same', repeated: 49, at: 51 }),
      expect.objectContaining({ message: 'hot path', detail: 'Error: other', repeated: 0 }),
    ]);
  });

  it('forgets what was logged while a private window was open, once the last one closes', () => {
    const diag = new Diagnostics();
    const changes: number[] = [];
    diag.onChange(() => changes.push(diag.entries().length));
    diag.log('warn', 'hub', 'before');
    diag.setPrivateSession(true);
    diag.log('warn', 'media', 'during');
    expect(diag.entries().map((e) => e.private)).toEqual([false, true]);
    diag.setPrivateSession(false);
    expect(diag.entries().map((e) => e.message)).toEqual(['before']);
    expect(changes).toEqual([1, 2, 1]);
  });

  it('drops the private entries it restores while no private window is open', () => {
    const diag = new Diagnostics({ now: () => 3 });
    diag.setPrivateSession(false);
    const changes: number[] = [];
    diag.onChange(() => changes.push(diag.entries().length));
    diag.restore([saved(1, 'hub', 'kept'), saved(2, 'hub', 'private', true)]);
    expect(diag.entries().map((e) => e.message)).toEqual(['kept']);
    expect(changes).toEqual([1]);
  });

  it('forgets private entries whenever it hears no private window is open, not only on a change', () => {
    const diag = new Diagnostics();
    diag.setPrivateSession(null);
    diag.restore([saved(1, 'hub', 'private', true)]);
    diag.log('warn', 'hub', 'unknown');
    diag.setPrivateSession(false);
    expect(diag.entries()).toEqual([
      expect.objectContaining({ message: 'unknown', private: false }),
    ]);
    diag.setPrivateSession(false);
    expect(diag.entries()).toHaveLength(1);
  });

  it('keeps private what it logged before it knew, once it hears a private window is open', () => {
    const diag = new Diagnostics();
    diag.setPrivateSession(null);
    diag.log('warn', 'hub', 'unknown');
    diag.setPrivateSession(true);
    diag.setPrivateSession(false);
    expect(diag.entries()).toEqual([]);
  });

  it('does not count a repeat into a restored private entry before it knows of the windows', () => {
    const diag = new Diagnostics({ now: () => 5 });
    diag.setPrivateSession(null);
    diag.restore([{ ...saved(1, 'hub', 'same', true), repeated: 7 }]);
    diag.log('warn', 'hub', 'same');
    diag.setPrivateSession(false);
    expect(diag.entries()).toEqual([
      expect.objectContaining({ message: 'same', repeated: 0, private: false, at: 5 }),
    ]);
  });

  it('logs the error a fallback handles and answers its value', async () => {
    diagnostics.clear();
    const value = await Promise.reject(new Error('gone')).catch(fallback('hub', 'Read failed', 7));
    expect(value).toBe(7);
    expect(diagnostics.entries()).toEqual([
      expect.objectContaining({ level: 'warn', message: 'Read failed', detail: 'Error: gone' }),
    ]);
  });
});

/** An entry as a context saved it. */
function saved(
  at: number,
  scope: 'hub' | 'popup' | 'settings',
  message = String(at),
  isPrivate = false,
): DiagEntry {
  return { at, level: 'warn', scope, message, detail: null, repeated: 0, private: isPrivate };
}

class MemoryStore implements DiagStore {
  readonly items = new Map<string, unknown>();
  failWrites = false;
  writes = 0;

  get(key: string): Promise<Record<string, unknown>> {
    return Promise.resolve(this.items.has(key) ? { [key]: this.items.get(key) } : {});
  }

  set(items: Record<string, unknown>): Promise<void> {
    this.writes += 1;
    if (this.failWrites) return Promise.reject(new Error('quota'));
    for (const [key, value] of Object.entries(items)) this.items.set(key, value);
    return Promise.resolve();
  }
}

describe('persistDiagnostics', () => {
  it('restores what an earlier run kept, skipping what does not read as an entry', async () => {
    const store = new MemoryStore();
    const kept = saved(1, 'hub', 'old');
    store.items.set(`${DIAG_KEY_PREFIX}background`, [kept, { at: 'x' }, 'junk']);
    const diag = new Diagnostics();
    await persistDiagnostics(diag, store, 'background');
    expect(diag.entries()).toEqual([kept]);
  });

  it('saves every new entry, and survives a failing store', async () => {
    const store = new MemoryStore();
    const diag = new Diagnostics();
    await persistDiagnostics(diag, store, 'popup');
    diag.log('warn', 'popup', 'one');
    diag.log('warn', 'popup', 'two');
    await flush();
    expect(store.items.get(`${DIAG_KEY_PREFIX}popup`)).toEqual(diag.entries());
    store.failWrites = true;
    diag.log('error', 'popup', 'three');
    await flush();
    expect(diag.entries()).toHaveLength(3);
  });

  it('reads every context, the options page included, oldest first', async () => {
    const store = new MemoryStore();
    store.items.set(`${DIAG_KEY_PREFIX}background`, [saved(1, 'hub'), saved(4, 'hub')]);
    store.items.set(`${DIAG_KEY_PREFIX}popup`, [saved(2, 'popup')]);
    store.items.set(`${DIAG_KEY_PREFIX}options`, [saved(3, 'settings')]);
    expect((await readDiagnostics(store)).map((e) => e.at)).toEqual([1, 2, 3, 4]);
  });

  it('keeps what an earlier run saved when an entry comes before it is read back', async () => {
    const store = new MemoryStore();
    store.items.set(`${DIAG_KEY_PREFIX}background`, [saved(1, 'hub', 'old')]);
    const diag = new Diagnostics({ now: () => 2 });
    const persisting = persistDiagnostics(diag, store, 'background');
    diag.log('warn', 'hub', 'early');
    await persisting;
    await flush();
    const stored = store.items.get(`${DIAG_KEY_PREFIX}background`);
    expect(stored).toEqual([
      expect.objectContaining({ message: 'old' }),
      expect.objectContaining({ message: 'early' }),
    ]);
  });

  it('removes the private entries the other contexts saved, and leaves its own to its writer', async () => {
    const store = new MemoryStore();
    const own = [saved(1, 'hub'), saved(2, 'hub', 'p', true)];
    store.items.set(`${DIAG_KEY_PREFIX}background`, own);
    store.items.set(`${DIAG_KEY_PREFIX}popup`, [saved(3, 'popup'), saved(4, 'popup', 'p', true)]);
    store.items.set(`${DIAG_KEY_PREFIX}options`, [
      saved(5, 'settings'),
      saved(6, 'settings', 'p', true),
    ]);
    await forgetPrivate(store, 'background');
    expect(store.items.get(`${DIAG_KEY_PREFIX}background`)).toBe(own);
    expect((await readDiagnostics(store)).map((e) => e.at)).toEqual([1, 2, 3, 5]);
  });
});

describe('logUncaught', () => {
  it('logs uncaught errors and unhandled rejections', () => {
    const listeners = new Map<string, (event: { error?: unknown; reason?: unknown }) => void>();
    const target: ErrorEvents = {
      addEventListener: (type: string, listener: (event: object) => void) => {
        listeners.set(type, listener);
      },
    };
    logUncaught(target, 'background');
    diagnostics.clear();
    listeners.get('error')?.({ error: new Error('thrown') });
    listeners.get('unhandledrejection')?.({ reason: new Error('rejected') });
    expect(diagnostics.entries()).toEqual([
      expect.objectContaining({ level: 'error', scope: 'background', detail: 'Error: thrown' }),
      expect.objectContaining({ level: 'error', scope: 'background', detail: 'Error: rejected' }),
    ]);
  });
});

describe('writeToConsole', () => {
  it('writes each level to its own console method', () => {
    const spies = {
      error: vi.spyOn(console, 'error').mockImplementation(() => undefined),
      warn: vi.spyOn(console, 'warn').mockImplementation(() => undefined),
      info: vi.spyOn(console, 'info').mockImplementation(() => undefined),
      debug: vi.spyOn(console, 'debug').mockImplementation(() => undefined),
    };
    for (const level of ['error', 'warn', 'info', 'debug'] as const) {
      writeToConsole({
        at: 0,
        level,
        scope: 'hub',
        message: 'm',
        detail: level === 'error' ? 'd' : null,
        repeated: 0,
        private: false,
      });
    }
    expect(spies.error).toHaveBeenCalledWith('[wirecatch:hub] m', 'd');
    expect(spies.warn).toHaveBeenCalledWith('[wirecatch:hub] m');
    expect(spies.info).toHaveBeenCalledOnce();
    expect(spies.debug).toHaveBeenCalledOnce();
  });

  it('is not used by the tests themselves', () => {
    const spy = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    log.warn('hub', 'quiet');
    expect(spy).not.toHaveBeenCalled();
  });
});
