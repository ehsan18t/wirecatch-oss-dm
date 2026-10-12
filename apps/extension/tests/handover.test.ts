/**
 * The most important rule: no download is ever lost. These tests run the real hub,
 * handover and native client against a fake browser downloads API and a fake port.
 */
import type { NewDownload } from '@wirecatch/protocol';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CookieContext } from '../src/lib/cookies';
import type { CachedSettings, SettingsCache } from '../src/lib/hub/deps';
import { Hub } from '../src/lib/hub/hub';
import type { BackgroundMessage, PopupRequest } from '../src/lib/messages';
import { NativeClient } from '../src/lib/native/client';
import {
  DEFAULT_SETTINGS,
  FakeApp,
  FakeDownloads,
  MemoryCache,
  MemoryInFlight,
  MemoryNotice,
  channelPair,
  downloadItem,
  flush,
} from './fakes';

interface Options {
  configure?: (app: FakeApp, cache: MemoryCache) => void;
  siteAccess?: boolean;
  inFlight?: MemoryInFlight;
  notice?: MemoryNotice;
  cache?: SettingsCache;
  downloads?: FakeDownloads;
  start?: boolean;
}

interface Setup {
  app: FakeApp;
  downloads: FakeDownloads;
  client: NativeClient;
  hub: Hub;
  inFlight: MemoryInFlight;
  notice: MemoryNotice;
  /** Every text the toolbar badge was given, in order. */
  badges: string[];
  cookieCalls: [string, string | null][];
  cookieContexts: CookieContext[];
  started: Promise<void>;
}

async function setup(options: Options = {}): Promise<Setup> {
  const app = new FakeApp();
  const memory = new MemoryCache();
  options.configure?.(app, memory);
  const downloads = options.downloads ?? new FakeDownloads();
  const inFlight = options.inFlight ?? new MemoryInFlight();
  const notice = options.notice ?? new MemoryNotice();
  const badges: string[] = [];
  const cookieCalls: [string, string | null][] = [];
  const cookieContexts: CookieContext[] = [];
  const client = new NativeClient({
    transport: app,
    hello: () =>
      Promise.resolve({
        client: 'extension',
        client_version: '0.1.0',
        protocol_version: 1,
        browser: null,
      }),
    backoffMs: [60_000],
  });
  let next = 0;
  const hub = new Hub({
    client,
    cache: options.cache ?? memory,
    downloads,
    cookieHeader: (url, storeId, context) => {
      cookieCalls.push([url, storeId]);
      cookieContexts.push(context);
      return Promise.resolve(url.includes('example.com') ? 'session=abc' : null);
    },
    hasSiteAccess: () => Promise.resolve(options.siteAccess ?? true),
    userAgent: 'TestBrowser/1.0',
    inFlight,
    notice,
    badge: { set: (text) => void badges.push(text) },
    newHandoverId: () => `h${String((next += 1))}`,
    timing: { connectWaitMs: 3_000, requestTimeoutMs: 5_000, withdrawTimeoutMs: 3_000 },
  });
  const started = hub.start();
  if (options.start !== false) {
    await started;
    await flush();
  }
  return {
    app,
    downloads,
    client,
    hub,
    inFlight,
    notice,
    badges,
    cookieCalls,
    cookieContexts,
    started,
  };
}

function addDownloadCalls(app: FakeApp): NewDownload[] {
  return app
    .calls('add_download')
    .map((c) => (c.method === 'add_download' ? c.params : (null as never)));
}

let s: Setup;

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  s.client.stop();
  vi.useRealTimers();
});

describe('handover', () => {
  it('pauses, hands over everything the app needs, then cancels and erases only after the app confirms', async () => {
    s = await setup();
    let logWhenAppAsked: string[] = [];
    s.app.onRequest = (call) => {
      if (call.method === 'add_download') logWhenAppAsked = [...s.downloads.log];
    };

    const result = await s.hub.onDownload(downloadItem({ id: 42 }));

    expect(logWhenAppAsked).toEqual(['pause:42']);
    expect(s.downloads.log).toEqual(['pause:42', 'cancel:42', 'erase:42']);
    expect(result).toEqual({
      kind: 'handed_over',
      handoverId: 'h1',
      action: 'started',
      browserCopyRemoved: true,
    });
    expect(addDownloadCalls(s.app)).toEqual([
      {
        handover_id: 'h1',
        url: 'https://files.example.com/big.zip',
        referrer: 'https://example.com/downloads',
        page_url: 'https://example.com/downloads',
        suggested_name: 'big.zip',
        mime: 'application/zip',
        size_hint: 50_000_000,
        cookies: 'session=abc',
        user_agent: 'TestBrowser/1.0',
        extra_headers: [],
        source: 'browser_download',
        queue: 1,
      },
    ]);
  });

  it('leaves a download the browser flags untouched', async () => {
    s = await setup();
    expect(await s.hub.onDownload(downloadItem({ id: 7, danger: 'file' }))).toBeNull();
    expect(s.downloads.log).toEqual([]);
    expect(s.app.calls('add_download')).toHaveLength(0);
  });

  it('keeps a download the browser flags while it is paused, and resumes it', async () => {
    const downloads = new FakeDownloads();
    downloads.browserState = { state: 'in_progress', paused: true, danger: 'url' };
    s = await setup({ downloads });
    const result = await s.hub.onDownload(downloadItem({ id: 7, danger: 'safe' }));
    expect(result).toMatchObject({
      kind: 'kept',
      reason: 'browser_flagged',
      browserCopy: 'resumed',
    });
    expect(downloads.log).toEqual(['pause:7', 'resume:7']);
    expect(s.app.calls('add_download')).toHaveLength(0);
  });

  it('leaves programs in the browser untouched, so its own checks run', async () => {
    s = await setup();
    const item = downloadItem({
      id: 8,
      filename: '/home/me/Downloads/setup.exe',
      mime: 'application/octet-stream',
    });
    expect(await s.hub.onDownload(item)).toBeNull();
    expect(s.downloads.log).toEqual([]);
    expect(s.app.calls('add_download')).toHaveLength(0);
  });

  it('reads cookies as for a navigation started by the page the download came from', async () => {
    s = await setup();
    await s.hub.onDownload(
      downloadItem({
        url: 'https://example.com/get?id=1',
        finalUrl: 'https://files.example.com/big.zip',
      }),
    );
    expect(s.cookieContexts).toEqual([
      {
        initiator: 'https://example.com/downloads',
        originalUrl: 'https://example.com/get?id=1',
        navigation: true,
      },
    ]);
  });

  it('uses a new handover id for every download', async () => {
    s = await setup();
    await s.hub.onDownload(downloadItem({ id: 1 }));
    await s.hub.onDownload(downloadItem({ id: 2 }));
    expect(addDownloadCalls(s.app).map((d) => d.handover_id)).toEqual(['h1', 'h2']);
  });

  it.each(['queued', 'awaiting_confirmation'] as const)(
    'treats "%s" as accepted and removes the browser copy',
    async (action) => {
      s = await setup({
        configure: (app) => {
          app.addBehavior = action;
        },
      });
      const result = await s.hub.onDownload(downloadItem());
      expect(result).toMatchObject({ kind: 'handed_over', action });
      expect(s.downloads.log).toEqual(['pause:1', 'cancel:1', 'erase:1']);
    },
  );

  it('resumes the browser download when the app answers with an error, without withdrawing', async () => {
    s = await setup({
      configure: (app) => {
        app.addBehavior = {
          error: { code: 'invalid_url', message: 'That address cannot be downloaded.' },
        };
      },
    });
    const result = await s.hub.onDownload(downloadItem());
    expect(s.downloads.log).toEqual(['pause:1', 'resume:1']);
    expect(result).toEqual({
      kind: 'kept',
      handoverId: 'h1',
      reason: 'app_refused',
      detail: 'That address cannot be downloaded.',
      browserCopy: 'resumed',
      uncertain: false,
    });
    expect(s.app.calls('withdraw_download')).toHaveLength(0);
    expect(s.hub.snapshot().notice?.message).toBe(
      'big.zip stayed in the browser: That address cannot be downloaded.',
    );
  });

  it('resumes the browser download on an answer it does not understand', async () => {
    s = await setup({
      configure: (app) => {
        app.addBehavior = 'unexpected_action';
      },
    });
    const result = await s.hub.onDownload(downloadItem());
    expect(s.downloads.log).toEqual(['pause:1', 'resume:1']);
    expect(result).toMatchObject({ kind: 'kept', reason: 'app_refused' });
  });

  it('fails one oversized request on its own and keeps the connection', async () => {
    s = await setup();
    const hugeCookieSite = 'https://example.com/' + 'x'.repeat(1024 * 1024);
    const result = await s.hub.onDownload(
      downloadItem({ url: hugeCookieSite, finalUrl: hugeCookieSite }),
    );
    expect(result).toMatchObject({ kind: 'kept', reason: 'app_refused', browserCopy: 'resumed' });
    if (result?.kind === 'kept') expect(result.detail).toContain('too large');
    expect(s.app.calls('add_download')).toHaveLength(0);
    expect(s.app.calls('withdraw_download')).toHaveLength(0);
    expect(s.client.status.state).toBe('connected');
  });

  describe('when the answer does not come back', () => {
    it('times out, withdraws, and resumes the browser copy on "withdrawn"', async () => {
      s = await setup({
        configure: (app) => {
          app.addBehavior = 'hang';
          app.withdrawBehavior = 'withdrawn';
        },
      });
      const pending = s.hub.onDownload(downloadItem());
      await vi.advanceTimersByTimeAsync(4_999);
      expect(s.downloads.log).toEqual(['pause:1']);
      expect(s.app.calls('withdraw_download')).toHaveLength(0);
      await vi.advanceTimersByTimeAsync(1);
      const result = await pending;
      expect(s.app.calls('withdraw_download')).toEqual([
        { method: 'withdraw_download', params: { handover_id: 'h1' } },
      ]);
      expect(s.downloads.log).toEqual(['pause:1', 'resume:1']);
      expect(result).toMatchObject({
        kind: 'kept',
        reason: 'no_answer',
        browserCopy: 'resumed',
        uncertain: false,
      });
    });

    it('resumes the browser copy on "not_found"', async () => {
      s = await setup({
        configure: (app) => {
          app.addBehavior = 'hang';
          app.withdrawBehavior = 'not_found';
        },
      });
      const pending = s.hub.onDownload(downloadItem());
      await vi.advanceTimersByTimeAsync(5_000);
      expect(await pending).toMatchObject({ kind: 'kept', reason: 'no_answer', uncertain: false });
      expect(s.downloads.log).toEqual(['pause:1', 'resume:1']);
    });

    it('cancels and erases the browser copy on "already_started"', async () => {
      s = await setup({
        configure: (app) => {
          app.addBehavior = 'hang';
          app.withdrawBehavior = 'already_started';
        },
      });
      const pending = s.hub.onDownload(downloadItem());
      await vi.advanceTimersByTimeAsync(5_000);
      expect(await pending).toEqual({
        kind: 'handed_over',
        handoverId: 'h1',
        action: 'started',
        browserCopyRemoved: true,
      });
      expect(s.downloads.log).toEqual(['pause:1', 'cancel:1', 'erase:1']);
    });

    it('resumes the browser copy when the withdraw itself times out (two copies beat none)', async () => {
      s = await setup({
        configure: (app) => {
          app.addBehavior = 'hang';
          app.withdrawBehavior = 'hang';
        },
      });
      const pending = s.hub.onDownload(downloadItem());
      await vi.advanceTimersByTimeAsync(5_000);
      expect(s.downloads.log).toEqual(['pause:1']);
      await vi.advanceTimersByTimeAsync(3_000);
      expect(await pending).toMatchObject({
        kind: 'kept',
        reason: 'no_answer',
        browserCopy: 'resumed',
        uncertain: true,
      });
      expect(s.downloads.log).toEqual(['pause:1', 'resume:1']);
    });

    it('resumes the browser copy when the withdraw fails', async () => {
      s = await setup({
        configure: (app) => {
          app.addBehavior = 'hang';
          app.withdrawBehavior = { error: { code: 'internal', message: 'Database locked.' } };
        },
      });
      const pending = s.hub.onDownload(downloadItem());
      await vi.advanceTimersByTimeAsync(5_000);
      expect(await pending).toMatchObject({
        kind: 'kept',
        reason: 'no_answer',
        uncertain: true,
        detail: 'Database locked.',
      });
      expect(s.downloads.log).toEqual(['pause:1', 'resume:1']);
    });

    it('warns about a possible duplicate when the app confirms after the browser kept its copy', async () => {
      s = await setup({
        configure: (app) => {
          app.addBehavior = 'started';
          app.addLateMs = 9_000; // after the 5 s timeout and the 3 s withdraw
          app.withdrawBehavior = 'hang';
        },
      });
      const pending = s.hub.onDownload(downloadItem());
      await vi.advanceTimersByTimeAsync(8_000);
      await pending;
      expect(s.hub.snapshot().notice?.message).toBe(
        'big.zip stayed in the browser: Wirecatch did not answer in time.',
      );
      await vi.advanceTimersByTimeAsync(1_000);
      expect(s.hub.snapshot().notice?.message).toBe(
        'big.zip may be downloading twice: Wirecatch took it after all, and the browser kept its copy too. You can cancel one of them.',
      );
    });

    it('warns at once when the late confirmation came while the withdraw was still waiting', async () => {
      s = await setup({
        configure: (app) => {
          app.addBehavior = 'started';
          app.addLateMs = 6_000; // during the withdraw
          app.withdrawBehavior = 'hang';
        },
      });
      const pending = s.hub.onDownload(downloadItem());
      await vi.advanceTimersByTimeAsync(8_000);
      await pending;
      expect(s.downloads.log).toEqual(['pause:1', 'resume:1']);
      expect(s.hub.snapshot().notice?.message).toContain('may be downloading twice');
    });

    it('does not warn when the app withdrew the download before its late answer arrived', async () => {
      s = await setup({
        configure: (app) => {
          app.addBehavior = 'started';
          app.addLateMs = 9_000;
          app.withdrawBehavior = 'withdrawn';
        },
      });
      const pending = s.hub.onDownload(downloadItem());
      await vi.advanceTimersByTimeAsync(5_000);
      await pending;
      await vi.advanceTimersByTimeAsync(5_000);
      expect(s.hub.snapshot().notice?.message).not.toContain('twice');
    });

    it('withdraws over a new connection when the connection broke mid-handover', async () => {
      s = await setup({
        configure: (app) => {
          app.addBehavior = 'disconnect';
          app.withdrawBehavior = 'withdrawn';
        },
      });
      const result = await s.hub.onDownload(downloadItem());
      expect(s.app.ports).toHaveLength(2);
      expect(s.app.ports[1]?.methods.slice(0, 1)).toEqual(['hello']);
      expect(s.app.ports[1]?.methods).toContain('withdraw_download');
      expect(s.downloads.log).toEqual(['pause:1', 'resume:1']);
      expect(result).toMatchObject({ kind: 'kept', reason: 'no_answer', uncertain: false });
    });
  });

  describe('when the app cannot be reached', () => {
    it('resumes the browser download when nothing is known', async () => {
      s = await setup({
        configure: (app) => {
          app.hostMissing = 'Specified native messaging host not found.';
        },
      });
      const result = await s.hub.onDownload(downloadItem());
      expect(s.downloads.log).toEqual(['pause:1', 'resume:1']);
      expect(result).toMatchObject({
        kind: 'kept',
        reason: 'app_unreachable',
        browserCopy: 'resumed',
      });
      expect(s.app.calls('add_download')).toHaveLength(0);
    });

    it('does not even pause a download the cached settings exclude', async () => {
      s = await setup({
        configure: (app, cache) => {
          cache.value = {
            settings: { ...DEFAULT_SETTINGS, capture_enabled: false },
            queues: [],
            savedAt: 1,
            appClosed: false,
          };
          app.hostMissing = 'Specified native messaging host not found.';
        },
      });
      expect(await s.hub.onDownload(downloadItem())).toBeNull();
      expect(s.downloads.log).toEqual([]);
    });

    it('resumes a download the cached settings would capture', async () => {
      s = await setup({
        configure: (app, cache) => {
          cache.value = { settings: DEFAULT_SETTINGS, queues: [], savedAt: 1, appClosed: false };
          app.hostMissing = 'Specified native messaging host not found.';
        },
      });
      const result = await s.hub.onDownload(downloadItem());
      expect(s.downloads.log).toEqual(['pause:1', 'resume:1']);
      expect(result).toMatchObject({ kind: 'kept', reason: 'app_unreachable' });
    });

    it('judges by settings the app sent while the settings request was waiting', async () => {
      s = await setup({
        configure: (app) => {
          // Connected, but the app stops answering calls from its first settings request on.
          app.onRequest = (call) => {
            if (call.method === 'get_extension_settings') app.answers = false;
          };
        },
      });
      const pending = s.hub.onDownload(downloadItem());
      await flush();
      // An event still arrives while the request waits: it is the newest copy, and it says no.
      s.app.changeSettings({ capture_enabled: false });
      await vi.advanceTimersByTimeAsync(3_000);
      const result = await pending;
      expect(result).toMatchObject({ kind: 'kept', reason: 'capture_off', browserCopy: 'resumed' });
      expect(s.app.calls('add_download')).toHaveLength(0);
    });

    it('resumes the browser download when the bridge cannot start the app', async () => {
      s = await setup({
        configure: (app) => {
          app.helloError = { code: 'app_unavailable', message: 'Could not start Wirecatch.' };
        },
      });
      const result = await s.hub.onDownload(downloadItem());
      expect(s.downloads.log).toEqual(['pause:1', 'resume:1']);
      expect(result).toMatchObject({ kind: 'kept', reason: 'app_unreachable' });
    });

    it('waits for a connection that is still starting, then hands over', async () => {
      s = await setup({
        configure: (app) => {
          app.hostMissing = 'Specified native messaging host not found.';
        },
      });
      s.app.hostMissing = null;
      const result = await s.hub.onDownload(downloadItem());
      expect(result).toMatchObject({ kind: 'handed_over' });
      expect(s.downloads.log).toEqual(['pause:1', 'cancel:1', 'erase:1']);
    });
  });

  // Every wake of the background (any web response, any tab change) started a bridge process, also
  // while Wirecatch was closed, only to learn it was still closed.
  it('starts no bridge on waking while the app was closed when last seen', async () => {
    s = await setup({
      configure: (fake, cache) => {
        fake.running = false;
        cache.value = { settings: DEFAULT_SETTINGS, queues: [], savedAt: 1, appClosed: true };
      },
    });
    expect(s.app.ports).toHaveLength(0);
    // The popup shows whether the app runs, so it finds out.
    const [, background] = channelPair<PopupRequest, BackgroundMessage>();
    s.hub.addPopup(background);
    await flush();
    expect(s.app.ports).toHaveLength(1);
    expect(s.app.starts).toBe(0);
  });

  // Opening the popup replaced the port a download had opened to start the app with one that
  // may not start it: the bridge said the app was not running, and the download stayed in the browser.
  it('keeps the connection a download is starting the app with when the popup opens', async () => {
    s = await setup({
      configure: (fake, cache) => {
        fake.running = false;
        fake.startDelayMs = 1_000;
        cache.value = { settings: DEFAULT_SETTINGS, queues: [], savedAt: 1, appClosed: true };
      },
    });
    const download = s.hub.onDownload(downloadItem());
    await flush();
    expect(s.app.ports).toHaveLength(1);

    const [, background] = channelPair<PopupRequest, BackgroundMessage>();
    s.hub.addPopup(background);
    await flush();
    await vi.advanceTimersByTimeAsync(1_000);

    expect(await download).toMatchObject({ kind: 'handed_over' });
    expect(s.app.ports).toHaveLength(1);
    expect(s.app.starts).toBe(1);
  });

  // Opening the popup dropped the live connection a download was being handed over on, so its
  // answer never came: the download waited out its time and was withdrawn.
  it('keeps the live connection a download is being handed over on when the popup opens', async () => {
    s = await setup({
      configure: (fake, cache) => {
        fake.running = false;
        cache.value = { settings: DEFAULT_SETTINGS, queues: [], savedAt: 1, appClosed: true };
      },
    });
    const [, background] = channelPair<PopupRequest, BackgroundMessage>();
    s.app.onRequest = (call) => {
      if (call.method === 'add_download') s.hub.addPopup(background);
    };

    const download = s.hub.onDownload(downloadItem());
    await flush();
    // Long enough for the request to time out and be withdrawn, as it did.
    await vi.advanceTimersByTimeAsync(20_000);

    expect(await download).toMatchObject({ kind: 'handed_over' });
    expect(s.app.ports).toHaveLength(1);
    expect(s.app.calls('withdraw_download')).toHaveLength(0);
  });

  it('remembers that the app is closed, and forgets it once the app runs', async () => {
    const cache = new MemoryCache();
    cache.value = { settings: DEFAULT_SETTINGS, queues: [], savedAt: 1, appClosed: false };
    const { app, client } = await setup({
      cache,
      configure: (fake) => {
        fake.running = false;
      },
    });
    expect(client.status.state).toBe('unavailable');
    expect(cache.value.appClosed).toBe(true);

    app.running = true;
    client.reconnectNow();
    await flush();
    expect(cache.value.appClosed).toBe(false);
  });

  it('judges nothing before the cached settings are loaded', async () => {
    let release: (value: CachedSettings | null) => void = () => undefined;
    const slowCache: SettingsCache = {
      load: () =>
        new Promise((resolve) => {
          release = resolve;
        }),
      save: () => Promise.resolve(),
    };
    s = await setup({
      cache: slowCache,
      start: false,
      configure: (app) => {
        app.hostMissing = 'Specified native messaging host not found.';
      },
    });
    const pending = s.hub.onDownload(downloadItem());
    await flush();
    expect(s.downloads.log).toEqual([]);
    release({
      settings: { ...DEFAULT_SETTINGS, capture_enabled: false },
      queues: [],
      savedAt: 1,
      appClosed: false,
    });
    expect(await pending).toBeNull();
    expect(s.downloads.log).toEqual([]);
    await s.started;
  });

  it('leaves the download untouched when the browser will not pause it', async () => {
    s = await setup();
    s.downloads.failing.add('pause');
    const result = await s.hub.onDownload(downloadItem());
    expect(s.downloads.log).toEqual(['pause:1']);
    expect(result).toMatchObject({
      kind: 'kept',
      reason: 'pause_failed',
      browserCopy: 'untouched',
    });
    expect(s.app.calls('add_download')).toHaveLength(0);
  });

  it('says how to recover when the browser will not resume a download that is still waiting', async () => {
    s = await setup({
      configure: (app) => {
        app.addBehavior = { error: { code: 'internal', message: 'Disk full.' } };
      },
    });
    s.downloads.failing.add('resume');
    s.downloads.browserState = { state: 'in_progress', paused: true };
    const result = await s.hub.onDownload(downloadItem());
    expect(result).toMatchObject({ kind: 'kept', browserCopy: 'stuck_paused' });
    expect(s.hub.snapshot().notice).toMatchObject({
      tone: 'error',
      message:
        "big.zip is paused in the browser. Open the browser's downloads page and click Resume.",
    });
  });

  it.each([
    ['cancelled by the person', { state: 'interrupted', paused: false }],
    ['removed from the list', null],
  ])('stays silent when the browser download was %s meanwhile', async (_label, state) => {
    s = await setup({
      configure: (app) => {
        app.addBehavior = { error: { code: 'internal', message: 'Disk full.' } };
      },
    });
    s.downloads.failing.add('resume');
    s.downloads.browserState = state;
    const result = await s.hub.onDownload(downloadItem());
    expect(result).toMatchObject({ kind: 'kept', browserCopy: 'gone' });
    expect(s.hub.snapshot().notice).toBeNull();
  });

  it('keeps the paused browser copy (and says so) when cancelling fails after the app took it', async () => {
    s = await setup();
    s.downloads.failing.add('cancel');
    const result = await s.hub.onDownload(downloadItem());
    expect(s.downloads.log).toEqual(['pause:1', 'cancel:1']);
    expect(result).toMatchObject({ kind: 'handed_over', browserCopyRemoved: false });
    expect(s.hub.snapshot().notice?.message).toContain('still in the browser');
  });

  it('rechecks the app settings after pausing and resumes if they now say no', async () => {
    s = await setup({
      configure: (app, cache) => {
        cache.value = { settings: { ...app.settings }, queues: [], savedAt: 1, appClosed: false };
        app.settings = { ...app.settings, capture_enabled: false };
        app.hostMissing = 'Specified native messaging host not found.';
      },
    });
    s.app.hostMissing = null;
    const result = await s.hub.onDownload(downloadItem());
    expect(s.downloads.log).toEqual(['pause:1', 'resume:1']);
    expect(result).toMatchObject({ kind: 'kept', reason: 'capture_off' });
  });

  it('does not touch downloads the settings exclude', async () => {
    s = await setup({
      configure: (app) => {
        app.settings = { ...app.settings, ignored_sites: ['example.com'] };
      },
    });
    expect(await s.hub.onDownload(downloadItem())).toBeNull();
    expect(
      await s.hub.onDownload(downloadItem({ url: 'blob:https://x.org/1', finalUrl: '' })),
    ).toBeNull();
    expect(s.downloads.log).toEqual([]);
  });

  it('keeps the download in the browser, untouched, when site access was revoked', async () => {
    s = await setup({ siteAccess: false });
    expect(await s.hub.onDownload(downloadItem())).toBeNull();
    expect(s.downloads.log).toEqual([]);
    expect(s.app.calls('add_download')).toHaveLength(0);
    expect(s.hub.snapshot().notice?.message).toContain('Allow access');
  });

  it('handles the same download only once at a time', async () => {
    s = await setup({
      configure: (app) => {
        app.addBehavior = 'hang';
      },
    });
    const first = s.hub.onDownload(downloadItem());
    expect(await s.hub.onDownload(downloadItem())).toBeNull();
    await vi.advanceTimersByTimeAsync(5_000);
    await first;
  });
});

describe('downloads left paused by a restarted background', () => {
  it('records the download before pausing it and clears it afterwards', async () => {
    s = await setup();
    let savedWhenPaused: number[] = [];
    const pause = s.downloads.pause.bind(s.downloads);
    s.downloads.pause = (id) => {
      savedWhenPaused = [...s.inFlight.ids];
      return pause(id);
    };
    await s.hub.onDownload(downloadItem({ id: 9 }));
    expect(savedWhenPaused).toEqual([9]);
    expect(s.inFlight.ids).toEqual([]);
  });

  it('does not write anything for downloads it does not take', async () => {
    s = await setup({
      configure: (app) => {
        app.settings = { ...app.settings, capture_enabled: false };
      },
    });
    await s.hub.onDownload(downloadItem());
    expect(s.inFlight.saves).toEqual([]);
  });

  it('resumes the ones still paused when it starts again', async () => {
    const inFlight = new MemoryInFlight();
    inFlight.ids = [5, 6];
    const downloads = new FakeDownloads();
    downloads.browserState = { state: 'in_progress', paused: true };
    s = await setup({ inFlight, downloads });
    expect(downloads.log).toEqual(['resume:5', 'resume:6']);
    expect(inFlight.ids).toEqual([]);
  });

  // The notice lived only in memory: when the browser stopped the background script, the next
  // wake cleared the badge and the person never learned the download was left paused.
  it('keeps a notice across a restart until it is dismissed', async () => {
    s = await setup({
      configure: (app) => {
        app.addBehavior = { error: { code: 'internal', message: 'Disk full.' } };
      },
    });
    s.downloads.failing.add('resume');
    await s.hub.onDownload(downloadItem());
    const shown = s.hub.snapshot().notice;
    expect(shown).toMatchObject({ tone: 'error' });
    s.client.stop();

    s = await setup({ notice: s.notice });
    expect(s.hub.snapshot().notice).toEqual(shown);
    expect(s.badges.at(-1)).toBe('!');

    const [popup, background] = channelPair<PopupRequest, BackgroundMessage>();
    s.hub.addPopup(background);
    popup.postMessage({ id: 1, command: { type: 'dismiss_notice' } });
    await flush();
    expect(s.notice.value).toBeNull();
  });

  it('leaves alone the ones that are no longer paused', async () => {
    const inFlight = new MemoryInFlight();
    inFlight.ids = [5];
    const downloads = new FakeDownloads();
    downloads.browserState = { state: 'complete', paused: false };
    s = await setup({ inFlight, downloads });
    expect(downloads.log).toEqual([]);
    expect(inFlight.ids).toEqual([]);
  });
});

describe('context menu', () => {
  it('sends the link with source context_menu and the tab cookie store', async () => {
    s = await setup();
    const result = await s.hub.onMenuClick(
      { linkUrl: 'https://example.com/file.iso', pageUrl: 'https://example.com/page' },
      { incognito: false, cookieStoreId: 'firefox-container-2' },
    );
    expect(result).toBe('handed_over');
    expect(addDownloadCalls(s.app)[0]).toMatchObject({
      handover_id: 'h1',
      url: 'https://example.com/file.iso',
      referrer: 'https://example.com/page',
      page_url: 'https://example.com/page',
      cookies: 'session=abc',
      source: 'context_menu',
      queue: 1,
    });
    expect(s.cookieCalls).toEqual([['https://example.com/file.iso', 'firefox-container-2']]);
    expect(s.cookieContexts).toEqual([{ initiator: 'https://example.com/page', navigation: true }]);
  });

  it('sends the referrer the browser would: the origin only to another origin, nothing from https to http', async () => {
    s = await setup();
    await s.hub.onMenuClick(
      { linkUrl: 'https://cdn.example.com/a.zip', pageUrl: 'https://example.com/account?id=7' },
      undefined,
    );
    await s.hub.onMenuClick(
      { linkUrl: 'http://mirror.example.com/a.zip', pageUrl: 'https://example.com/account?id=7' },
      undefined,
    );
    await s.hub.onMenuClick(
      { linkUrl: 'https://example.com/a.zip', pageUrl: 'https://example.com/account?id=7#files' },
      undefined,
    );
    expect(addDownloadCalls(s.app).map((d) => d.referrer)).toEqual([
      'https://example.com/',
      null,
      'https://example.com/account?id=7',
    ]);
    expect(s.downloads.log).toEqual([]);
  });

  it('uses the default cookie store in Chromium (no cookieStoreId)', async () => {
    s = await setup();
    await s.hub.onMenuClick(
      { linkUrl: 'https://example.com/a.zip' },
      { incognito: false, cookieStoreId: null },
    );
    expect(s.cookieCalls).toEqual([['https://example.com/a.zip', null]]);
  });

  it('skips private windows', async () => {
    s = await setup();
    const result = await s.hub.onMenuClick(
      { linkUrl: 'https://example.com/a.zip' },
      { incognito: true, cookieStoreId: null },
    );
    expect(result).toBe('private_window');
    expect(s.app.calls('add_download')).toHaveLength(0);
    expect(s.downloads.log).toEqual([]);
    expect(s.hub.snapshot().notice?.message).toContain('private windows');
  });

  it('lets the browser download it without site access, after the earlier checks', async () => {
    s = await setup({ siteAccess: false });
    expect(
      await s.hub.onMenuClick(
        { linkUrl: 'https://example.com/a.zip' },
        { incognito: true, cookieStoreId: null },
      ),
    ).toBe('private_window');
    expect(s.downloads.log).toEqual([]);
    const result = await s.hub.onMenuClick({ linkUrl: 'https://example.com/a.zip' }, undefined);
    expect(result).toBe('browser');
    expect(s.downloads.log).toEqual(['start:https://example.com/a.zip']);
    expect(s.app.calls('add_download')).toHaveLength(0);
    expect(s.hub.snapshot().notice?.message).toContain('Allow access');
  });

  it('uses the media address when there is no link', async () => {
    s = await setup();
    await s.hub.onMenuClick(
      { srcUrl: 'https://media.example.org/v.mp4', pageUrl: 'https://example.org/' },
      undefined,
    );
    expect(addDownloadCalls(s.app)[0]?.url).toBe('https://media.example.org/v.mp4');
  });

  it('lets the browser download it when the app does not take it', async () => {
    s = await setup({
      configure: (app) => {
        app.addBehavior = { error: { code: 'internal', message: 'Something broke.' } };
      },
    });
    const result = await s.hub.onMenuClick({ linkUrl: 'https://example.com/file.iso' }, undefined);
    expect(result).toBe('browser');
    expect(s.downloads.log).toEqual(['start:https://example.com/file.iso']);
    expect(s.hub.snapshot().notice?.message).toContain('the browser is downloading it instead');
  });

  it('withdraws after a timeout: "already_started" means the app has it', async () => {
    s = await setup({
      configure: (app) => {
        app.addBehavior = 'hang';
        app.withdrawBehavior = 'already_started';
      },
    });
    const pending = s.hub.onMenuClick({ linkUrl: 'https://example.com/file.iso' }, undefined);
    await vi.advanceTimersByTimeAsync(5_000);
    expect(await pending).toBe('handed_over');
    expect(s.downloads.log).toEqual([]);
  });

  it('withdraws after a timeout: "withdrawn" means the browser downloads it', async () => {
    s = await setup({
      configure: (app) => {
        app.addBehavior = 'hang';
        app.withdrawBehavior = 'withdrawn';
      },
    });
    const pending = s.hub.onMenuClick({ linkUrl: 'https://example.com/file.iso' }, undefined);
    await vi.advanceTimersByTimeAsync(5_000);
    expect(await pending).toBe('browser');
    expect(s.downloads.log).toEqual(['start:https://example.com/file.iso']);
  });

  it('lets the browser download programs, so its own checks run', async () => {
    s = await setup();
    const result = await s.hub.onMenuClick(
      { linkUrl: 'https://example.com/get/Setup.EXE' },
      undefined,
    );
    expect(result).toBe('browser');
    expect(s.app.calls('add_download')).toHaveLength(0);
    expect(s.downloads.log).toEqual(['start:https://example.com/get/Setup.EXE']);
    expect(s.hub.snapshot().notice?.message).toContain('is a program');
  });

  it('lets the browser download it when site access was revoked', async () => {
    s = await setup({ siteAccess: false });
    const result = await s.hub.onMenuClick({ linkUrl: 'https://example.com/file.iso' }, undefined);
    expect(result).toBe('browser');
    expect(s.app.calls('add_download')).toHaveLength(0);
  });

  it('lets the browser handle addresses only it can open', async () => {
    s = await setup();
    const result = await s.hub.onMenuClick({ srcUrl: 'data:image/png;base64,AAAA' }, undefined);
    expect(result).toBe('browser');
    expect(s.app.calls('add_download')).toHaveLength(0);
  });

  it('reports when neither the app nor the browser could download it', async () => {
    s = await setup({
      configure: (app) => {
        app.hostMissing = 'Specified native messaging host not found.';
      },
    });
    s.downloads.failing.add('start');
    const result = await s.hub.onMenuClick({ srcUrl: 'blob:https://example.com/123' }, undefined);
    expect(result).toBe('failed');
    expect(s.hub.snapshot().notice).toMatchObject({ tone: 'error' });
  });
});
