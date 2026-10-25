/** Errors the extension handles with a fallback still reach the diagnostics, redacted. */
import { afterEach, describe, expect, it } from 'vitest';
import { cookieHeaderFor } from '../src/lib/browser/adapters';
import { diagnostics, type DiagEntry } from '../src/lib/diag/log';
import { Hub } from '../src/lib/hub/hub';
import { NoticeBoard } from '../src/lib/hub/notice-board';
import { KnownSettings } from '../src/lib/hub/settings';
import { MediaCenter } from '../src/lib/media/center';
import { MediaRegistry } from '../src/lib/media/registry';
import { NativeClient } from '../src/lib/native/client';
import { parseUrl } from '../src/lib/url';
import {
  DEFAULT_SETTINGS,
  FakeApp,
  FakeDownloads,
  NORMAL_STORE,
  QUEUES,
  flush,
  stubGetAll,
} from './fakes';

/** The entries made while `run` runs. */
async function logged(run: () => Promise<void>): Promise<DiagEntry[]> {
  diagnostics.clear();
  await run();
  await flush();
  return diagnostics.entries();
}

const failing = (message: string) => (): Promise<never> => Promise.reject(new Error(message));

const stopped: NativeClient[] = [];
afterEach(() => {
  for (const c of stopped.splice(0)) c.stop();
});

describe('routed errors', () => {
  it('logs unreadable saved settings and paused downloads, and still starts', async () => {
    const client = new NativeClient({
      transport: new FakeApp(),
      hello: () => Promise.resolve({ client: 'extension', client_version: '0.1.0', browser: null }),
      backoffMs: [60_000],
    });
    stopped.push(client);
    const hub = new Hub({
      client,
      cache: { load: failing('cache gone'), save: () => Promise.resolve() },
      inFlight: { load: failing('session gone'), save: () => Promise.resolve() },
      downloads: new FakeDownloads(),
      cookieHeader: () => Promise.resolve(null),
      hasSiteAccess: () => Promise.resolve(true),
      userAgent: null,
    });
    const entries = await logged(() => hub.start());
    expect(entries).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ level: 'warn', scope: 'settings', detail: 'Error: cache gone' }),
        expect.objectContaining({ level: 'warn', scope: 'capture', detail: 'Error: session gone' }),
      ]),
    );
  });

  it('logs a session copy of the settings that cannot be written', async () => {
    const settings = new KnownSettings(
      { cache: { load: () => Promise.resolve(null), save: failing('quota') } },
      () => undefined,
    );
    const entries = await logged(async () => {
      settings.refreshed(DEFAULT_SETTINGS, null, QUEUES);
      await Promise.resolve();
    });
    expect(entries).toEqual([
      expect.objectContaining({ level: 'warn', scope: 'settings', detail: 'Error: quota' }),
    ]);
  });

  it('logs notices that cannot be read or saved', async () => {
    const board = new NoticeBoard({
      now: () => 1,
      changed: () => undefined,
      store: { load: failing('unreadable'), save: failing('quota') },
    });
    const entries = await logged(async () => {
      await board.load();
      board.warn('Something to say.');
    });
    expect(entries).toEqual([
      expect.objectContaining({ scope: 'notices', detail: 'Error: unreadable' }),
      expect.objectContaining({ scope: 'notices', detail: 'Error: quota' }),
    ]);
  });

  it('logs media lists and tabs that cannot be read, and redacts the address in the error', async () => {
    const center = new MediaCenter({
      privateWindowsOn: () => Promise.resolve(true),
      registry: new MediaRegistry(),
      getTab: () => Promise.resolve(null),
      send: () => Promise.resolve({ ok: true, message: '' }),
      store: {
        load: failing('no list for https://user:pw@video.example/watch?v=secret'),
        save: () => Promise.resolve(),
      },
      liveTabs: failing('tabs gone'),
    });
    const entries = await logged(() => center.start());
    expect(entries).toEqual([
      expect.objectContaining({
        scope: 'media',
        detail: 'Error: no list for https://video.example/…',
      }),
      expect.objectContaining({ scope: 'media', detail: 'Error: tabs gone' }),
    ]);
  });
});

describe('cookieHeaderFor', () => {
  const NAVIGATION = { initiator: null, navigation: true };

  const SID = { name: 'sid', value: '1', domain: 'a.example', path: '/' };

  it('logs nothing when a later way of reading the cookies works', async () => {
    stubGetAll((query) => {
      if (query.firstPartyDomain === undefined) throw new Error('first-party isolation');
      return [];
    });
    const entries = await logged(async () => {
      expect(await cookieHeaderFor('https://a.example/f', NORMAL_STORE, NAVIGATION)).toBeNull();
    });
    expect(entries).toEqual([]);
  });

  // Chromium 116 to 118, which the extension supports, know no partitions.
  it('still sends the unpartitioned cookies, and warns of nothing, when the browser cannot read partitions', async () => {
    stubGetAll((query) => {
      if (query.partitionKey !== undefined) throw new Error('unknown partitionKey');
      return [SID];
    });
    const entries = await logged(async () => {
      expect(await cookieHeaderFor('https://a.example/f', NORMAL_STORE, NAVIGATION)).toBe('sid=1');
    });
    expect(entries).toEqual([]);
  });

  it('warns when only the unpartitioned cookies cannot be read', async () => {
    stubGetAll((query) => {
      if (query.partitionKey === undefined) throw new Error('store gone');
      return [{ ...SID, partitionKey: { topLevelSite: 'https://a.example' } }];
    });
    const entries = await logged(async () => {
      expect(await cookieHeaderFor('https://a.example/f', NORMAL_STORE, NAVIGATION)).toBe('sid=1');
    });
    expect(entries).toEqual([
      expect.objectContaining({ level: 'warn', scope: 'capture', detail: 'Error: store gone' }),
    ]);
  });

  it('logs once when the browser rejects every way, and sends no cookies', async () => {
    stubGetAll(() => {
      throw new Error('no cookies here');
    });
    const entries = await logged(async () => {
      expect(await cookieHeaderFor('https://a.example/f', NORMAL_STORE, NAVIGATION)).toBeNull();
    });
    expect(entries).toEqual([
      expect.objectContaining({
        level: 'warn',
        scope: 'capture',
        detail: 'Error: no cookies here',
      }),
    ]);
  });
});

describe('parseUrl', () => {
  it('reads an absolute address, and answers null for anything else', () => {
    expect(parseUrl('https://a.example/x?y=1')?.hostname).toBe('a.example');
    expect(parseUrl('/relative/path')).toBeNull();
    expect(parseUrl('not an address')).toBeNull();
    expect(parseUrl('')).toBeNull();
  });
});
