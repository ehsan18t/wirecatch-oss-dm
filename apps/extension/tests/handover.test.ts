/**
 * The most important rule: no download is ever lost. These tests run the real hub,
 * handover and native client against a fake browser downloads API and a fake port.
 */
import { MAX_FRAME_LEN, type NewDownload } from '@wirecatch/protocol';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CookieContext, CookieStore } from '../src/lib/cookies';
import type { DownloadState } from '../src/lib/capture/handover';
import type { CachedSettings, InFlightEntry, SettingsCache } from '../src/lib/hub/deps';
import { Hub } from '../src/lib/hub/hub';
import { MENU_NOT_WEB } from '../src/lib/hub/notices';
import type { BackgroundMessage, Notice, PopupRequest } from '../src/lib/messages';
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
  cookieCalls: [string, CookieStore][];
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
  const cookieCalls: [string, CookieStore][] = [];
  const cookieContexts: CookieContext[] = [];
  const client = new NativeClient({
    transport: app,
    hello: () =>
      Promise.resolve({
        client: 'extension',
        client_version: '0.1.0',
        browser: null,
      }),
    backoffMs: [60_000],
  });
  let next = 0;
  const hub = new Hub({
    client,
    cache: options.cache ?? memory,
    downloads,
    cookieHeader: (url, store, context) => {
      cookieCalls.push([url, store]);
      cookieContexts.push(context);
      return Promise.resolve(url.includes('example.com') ? 'session=abc' : null);
    },
    hasSiteAccess: () => Promise.resolve(options.siteAccess ?? true),
    userAgent: 'TestBrowser/1.0',
    inFlight,
    notice,
    badge: { set: (text) => void badges.push(text) },
    newHandoverId: () => `h${String((next += 1))}`,
    timing: { connectWaitMs: 3000, requestTimeoutMs: 5000, withdrawTimeoutMs: 3000 },
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

function privateWindowsOff(app: FakeApp): void {
  app.settings = { ...app.settings, private_windows: false };
}

function latestNotice(hub: Hub): Notice | null {
  return hub.snapshot().notices[0] ?? null;
}

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
      browserCopy: 'removed',
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

  it('takes programs over like any other file (D60)', async () => {
    s = await setup();
    const item = downloadItem({
      id: 8,
      url: 'https://downloads.example.net/project/tool/setup.exe',
      filename: '/home/me/Downloads/setup.exe',
      mime: 'application/x-msdownload',
    });
    expect(await s.hub.onDownload(item)).toMatchObject({ kind: 'handed_over' });
    expect(s.app.calls('add_download')).toHaveLength(1);
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
    expect(latestNotice(s.hub)?.message).toBe(
      'big.zip stayed in the browser: That address cannot be downloaded.',
    );
  });

  it('withdraws after an answer it does not understand, and keeps the browser copy if withdrawn', async () => {
    s = await setup({
      configure: (app) => {
        app.addBehavior = 'unexpected_action';
      },
    });
    const result = await s.hub.onDownload(downloadItem());
    expect(s.downloads.log).toEqual(['pause:1', 'resume:1']);
    expect(s.app.calls('withdraw_download')).toHaveLength(1);
    // Told apart from a timeout: the app did answer, in a shape this version cannot read.
    expect(result).toMatchObject({ kind: 'kept', reason: 'app_unreadable', uncertain: false });
    expect(latestNotice(s.hub)?.message).toBe(
      'big.zip stayed in the browser: Wirecatch sent a response this version of the extension cannot read. Updating the extension may help.',
    );
  });

  it('counts a download as handed over when the withdraw after an unreadable answer says the app has it', async () => {
    s = await setup({
      configure: (app) => {
        app.addBehavior = 'unexpected_action';
        app.withdrawBehavior = 'already_started';
      },
    });
    const result = await s.hub.onDownload(downloadItem());
    expect(result).toMatchObject({ kind: 'handed_over', action: 'started' });
    expect(s.downloads.log).toEqual(['pause:1', 'cancel:1', 'erase:1']);
  });

  it('fails one oversized request on its own and keeps the connection', async () => {
    s = await setup();
    const hugeCookieSite = `https://example.com/${'x'.repeat(MAX_FRAME_LEN)}`;
    const result = await s.hub.onDownload(
      downloadItem({ url: hugeCookieSite, finalUrl: hugeCookieSite }),
    );
    expect(result).toMatchObject({ kind: 'kept', reason: 'app_refused', browserCopy: 'resumed' });
    if (result?.kind !== 'kept') throw new Error('the download was not kept in the browser');
    expect(result.detail).toContain('too large');
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
      await vi.advanceTimersByTimeAsync(4999);
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
      await vi.advanceTimersByTimeAsync(5000);
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
      await vi.advanceTimersByTimeAsync(5000);
      expect(await pending).toEqual({
        kind: 'handed_over',
        handoverId: 'h1',
        action: 'started',
        browserCopy: 'removed',
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
      await vi.advanceTimersByTimeAsync(5000);
      expect(s.downloads.log).toEqual(['pause:1']);
      await vi.advanceTimersByTimeAsync(3000);
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
      await vi.advanceTimersByTimeAsync(5000);
      expect(await pending).toMatchObject({
        kind: 'kept',
        reason: 'no_answer',
        uncertain: true,
        detail: 'Database locked.',
      });
      expect(s.downloads.log).toEqual(['pause:1', 'resume:1']);
    });

    it('counts a withdraw answer it does not understand as uncertain', async () => {
      s = await setup({
        configure: (app) => {
          app.addBehavior = 'hang';
          app.withdrawBehavior = 'unreadable';
        },
      });
      const pending = s.hub.onDownload(downloadItem());
      await vi.advanceTimersByTimeAsync(5000);
      expect(await pending).toMatchObject({
        kind: 'kept',
        reason: 'no_answer',
        browserCopy: 'resumed',
        uncertain: true,
      });
    });

    it('warns about a possible duplicate when a late answer cannot be read', async () => {
      s = await setup({
        configure: (app) => {
          app.addBehavior = 'unexpected_action';
          app.addLateMs = 9000;
          app.withdrawBehavior = 'hang';
        },
      });
      const pending = s.hub.onDownload(downloadItem());
      await vi.advanceTimersByTimeAsync(8000);
      expect(await pending).toMatchObject({ kind: 'kept', uncertain: true });
      await vi.advanceTimersByTimeAsync(1000);
      // Unreadable, so it may or may not be a confirmation: never "took it after all".
      expect(latestNotice(s.hub)?.message).toBe(
        'big.zip may be downloading twice: Wirecatch may have taken it, and the browser is downloading it too. If Wirecatch has it, cancel one of them.',
      );
    });

    it('warns about a possible duplicate when the app confirms after the browser kept its copy', async () => {
      s = await setup({
        configure: (app) => {
          app.addBehavior = 'started';
          app.addLateMs = 9000; // after the 5 s timeout and the 3 s withdraw
          app.withdrawBehavior = 'hang';
        },
      });
      const pending = s.hub.onDownload(downloadItem());
      await vi.advanceTimersByTimeAsync(8000);
      await pending;
      // A failed withdraw leaves both copies possible, and says so at once.
      expect(latestNotice(s.hub)?.message).toBe(
        'big.zip may be downloading twice: Wirecatch may have taken it, and the browser is downloading it too. If Wirecatch has it, cancel one of them.',
      );
      await vi.advanceTimersByTimeAsync(1000);
      expect(latestNotice(s.hub)?.message).toBe(
        'big.zip may be downloading twice: Wirecatch took it after all, and the browser kept its copy too. You can cancel one of them.',
      );
    });

    describe('a copy the browser will not resume, which the app may have', () => {
      const STUCK =
        "big.zip is paused in the browser, and Wirecatch may have taken it. Check Wirecatch's download list: if it is not there, open the browser's downloads page and click Resume.";
      const LEFT =
        "big.zip went to Wirecatch, but its paused copy is still in the browser's download list. You can remove it there.";

      async function stuckWithLateAnswer(
        addBehavior: 'started' | 'unexpected_action',
        addLateMs: number,
        meanwhile: () => void = () => undefined,
      ): Promise<void> {
        s = await setup({
          configure: (app) => {
            app.addBehavior = addBehavior;
            app.addLateMs = addLateMs;
            app.withdrawBehavior = 'hang';
          },
        });
        s.downloads.failing.add('resume');
        s.downloads.browserState = { state: 'in_progress', paused: true };
        const pending = s.hub.onDownload(downloadItem());
        await vi.advanceTimersByTimeAsync(8000);
        expect(await pending).toMatchObject({ browserCopy: 'stuck_paused', uncertain: true });
        meanwhile();
        await vi.advanceTimersByTimeAsync(1000);
      }

      /** The person resumes the copy, as the notice asks, before the late answer comes. */
      const resumedByThePerson = (): void => {
        s.downloads.states.set(1, { state: 'in_progress', paused: false });
      };

      it.each([
        ['interrupted', { state: 'interrupted', paused: false }],
        ['erased', null],
      ] as const)(
        'says nothing new when the copy was %s before a sure answer',
        async (_label, state) => {
          await stuckWithLateAnswer('started', 9000, () => {
            s.downloads.states.set(1, state === null ? null : { ...state });
          });
          expect(s.hub.snapshot().notices).toMatchObject([{ message: STUCK }]);
        },
      );

      it('warns of two copies when a sure answer comes after the person resumed the copy', async () => {
        await stuckWithLateAnswer('started', 9000, resumedByThePerson);
        expect(s.hub.snapshot().notices).toMatchObject([{ urgent: true }]);
        expect(latestNotice(s.hub)?.message).toContain('Wirecatch took it after all');
      });

      it('warns of two possible copies when an unreadable answer comes after the person resumed the copy', async () => {
        await stuckWithLateAnswer('unexpected_action', 9000, resumedByThePerson);
        expect(s.hub.snapshot().notices).toMatchObject([{ urgent: true }]);
        expect(latestNotice(s.hub)?.message).toContain(
          'Wirecatch may have taken it, and the browser is downloading it too',
        );
      });

      it('warns of two copies when a sure answer comes for a copy whose state cannot be read', async () => {
        s = await setup({
          configure: (app) => {
            app.addBehavior = 'started';
            app.addLateMs = 9000;
            app.withdrawBehavior = 'hang';
          },
        });
        s.downloads.failing.add('resume');
        s.downloads.failing.add('state');
        const pending = s.hub.onDownload(downloadItem());
        await vi.advanceTimersByTimeAsync(8000);
        expect(await pending).toMatchObject({ browserCopy: 'maybe_paused', uncertain: true });
        await vi.advanceTimersByTimeAsync(1000);
        expect(s.hub.snapshot().notices).toMatchObject([{ urgent: true }]);
        expect(latestNotice(s.hub)?.message).toContain('Wirecatch took it after all');
      });

      it.each([
        ['after the browser kept it', 9000],
        ['while the withdraw was waiting', 6000],
      ])(
        'keeps the notice to resume it when an unreadable late answer comes %s',
        async (_label, ms) => {
          await stuckWithLateAnswer('unexpected_action', ms);
          expect(s.hub.snapshot().notices).toMatchObject([
            { tone: 'error', urgent: true, untilResumed: true, message: STUCK },
          ]);
        },
      );

      it.each([
        ['after the browser kept it', 9000],
        ['while the withdraw was waiting', 6000],
      ])(
        'says the paused copy is left over when a sure confirmation comes %s',
        async (_label, ms) => {
          await stuckWithLateAnswer('started', ms);
          expect(s.hub.snapshot().notices).toMatchObject([{ message: LEFT }]);
        },
      );
    });

    it('warns of two possible copies when an unreadable answer came first and the person paused the copy', async () => {
      s = await setup({
        configure: (app) => {
          app.addBehavior = 'unexpected_action';
          app.addLateMs = 6000;
          app.withdrawBehavior = 'hang';
        },
      });
      s.downloads.afterCall = (call) => {
        if (call === 'resume:1') s.downloads.states.set(1, { state: 'in_progress', paused: true });
      };
      const pending = s.hub.onDownload(downloadItem());
      await vi.advanceTimersByTimeAsync(8000);
      expect(await pending).toMatchObject({ browserCopy: 'resumed', uncertain: true });
      expect(latestNotice(s.hub)?.message).toContain('may be downloading twice');
    });

    it('warns at once when the late confirmation came while the withdraw was still waiting', async () => {
      s = await setup({
        configure: (app) => {
          app.addBehavior = 'started';
          app.addLateMs = 6000; // during the withdraw
          app.withdrawBehavior = 'hang';
        },
      });
      const pending = s.hub.onDownload(downloadItem());
      await vi.advanceTimersByTimeAsync(8000);
      await pending;
      expect(s.downloads.log).toEqual(['pause:1', 'resume:1']);
      expect(latestNotice(s.hub)?.message).toContain('may be downloading twice');
    });

    it('does not warn when the app withdrew the download before its late answer arrived', async () => {
      s = await setup({
        configure: (app) => {
          app.addBehavior = 'started';
          app.addLateMs = 9000;
          app.withdrawBehavior = 'withdrawn';
        },
      });
      const pending = s.hub.onDownload(downloadItem());
      await vi.advanceTimersByTimeAsync(5000);
      await pending;
      await vi.advanceTimersByTimeAsync(5000);
      expect(latestNotice(s.hub)?.message).not.toContain('twice');
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
          cache.value = {
            settings: DEFAULT_SETTINGS,
            appClosed: false,
          };
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
      await vi.advanceTimersByTimeAsync(3000);
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
        cache.value = { settings: DEFAULT_SETTINGS, appClosed: true };
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

  it('keeps the connection a download is starting the app with when the popup opens', async () => {
    s = await setup({
      configure: (fake, cache) => {
        fake.running = false;
        fake.startDelayMs = 1000;
        cache.value = { settings: DEFAULT_SETTINGS, appClosed: true };
      },
    });
    const download = s.hub.onDownload(downloadItem());
    await flush();
    expect(s.app.ports).toHaveLength(1);

    const [, background] = channelPair<PopupRequest, BackgroundMessage>();
    s.hub.addPopup(background);
    await flush();
    await vi.advanceTimersByTimeAsync(1000);

    expect(await download).toMatchObject({ kind: 'handed_over' });
    expect(s.app.ports).toHaveLength(1);
    expect(s.app.starts).toBe(1);
  });

  it('keeps the live connection a download is being handed over on when the popup opens', async () => {
    s = await setup({
      configure: (fake, cache) => {
        fake.running = false;
        cache.value = { settings: DEFAULT_SETTINGS, appClosed: true };
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
    cache.value = { settings: DEFAULT_SETTINGS, appClosed: false };
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

  it('cancels the copy when the browser cannot say how it stands, and warns it may run twice', async () => {
    s = await setup();
    s.downloads.failing.add('state');
    const result = await s.hub.onDownload(downloadItem());
    expect(result).toMatchObject({ kind: 'handed_over', browserCopy: 'kept_running' });
    expect(s.downloads.log).toEqual(['pause:1', 'cancel:1']);
  });

  it('keeps a copy that finished just before the cancel, and warns of two copies', async () => {
    s = await setup();
    s.downloads.afterCall = (call) => {
      if (call === 'cancel:1') s.downloads.states.set(1, { state: 'complete', paused: false });
    };
    const result = await s.hub.onDownload(downloadItem());
    expect(result).toMatchObject({ kind: 'handed_over', browserCopy: 'kept_running' });
    expect(s.downloads.log).toEqual(['pause:1', 'cancel:1']);
    expect(latestNotice(s.hub)?.message).toContain('may be downloading twice');
  });

  it('shows an uncertain handover next to an unrelated error', async () => {
    s = await setup({
      configure: (app) => {
        app.addBehavior = 'hang';
        app.withdrawBehavior = 'hang';
      },
    });
    s.hub.notify('error', 'The video could not be sent.');
    const pending = s.hub.onDownload(downloadItem());
    await vi.advanceTimersByTimeAsync(8000);
    await pending;
    expect(s.hub.snapshot().notices.map((n) => n.tone)).toEqual(['warning', 'error']);
    expect(latestNotice(s.hub)?.message).toContain('may be downloading twice');
  });

  it('clears the notice asking to resume a download once the browser runs it', async () => {
    s = await setup({
      configure: (app) => {
        app.addBehavior = { error: { code: 'internal', message: 'Disk full.' } };
      },
    });
    s.downloads.failing.add('resume');
    await s.hub.onDownload(downloadItem({ id: 4 }));
    s.hub.notify('warning', 'Something else.');
    expect(s.hub.snapshot().notices).toHaveLength(2);
    s.hub.onDownloadMoved(4);
    expect(s.hub.snapshot().notices.map((n) => n.message)).toEqual(['Something else.']);
  });

  it('replaces the notice about a download with a newer one about it', async () => {
    s = await setup({
      configure: (app) => {
        app.addBehavior = { error: { code: 'internal', message: 'Disk full.' } };
      },
    });
    await s.hub.onDownload(downloadItem({ id: 4 }));
    await s.hub.onDownload(downloadItem({ id: 4 }));
    expect(s.hub.snapshot().notices).toHaveLength(1);
  });

  it('warns of two copies when a copy that cannot be read will not cancel', async () => {
    s = await setup();
    s.downloads.failing.add('state');
    s.downloads.failing.add('cancel');
    const result = await s.hub.onDownload(downloadItem());
    expect(result).toMatchObject({ kind: 'handed_over', browserCopy: 'kept_running' });
    expect(latestNotice(s.hub)?.message).toContain('may be downloading twice');
  });

  it('warns of two copies when the browser cannot say how a cancelled copy stands', async () => {
    s = await setup();
    s.downloads.afterCall = (call) => {
      if (call === 'cancel:1') s.downloads.failing.add('state');
    };
    const result = await s.hub.onDownload(downloadItem());
    expect(result).toMatchObject({ kind: 'handed_over', browserCopy: 'kept_running' });
    expect(latestNotice(s.hub)?.message).toContain('may be downloading twice');
  });

  it('counts a copy the person resumed during the withdraw as resumed when resume fails', async () => {
    s = await setup({
      configure: (app) => {
        app.addBehavior = 'hang';
        app.withdrawBehavior = 'withdrawn';
      },
    });
    s.downloads.failing.add('resume');
    s.app.onRequest = (call) => {
      if (call.method === 'withdraw_download') {
        s.downloads.browserState = { state: 'in_progress', paused: false };
      }
    };
    const pending = s.hub.onDownload(downloadItem());
    await vi.advanceTimersByTimeAsync(5000);
    expect(await pending).toMatchObject({ kind: 'kept', browserCopy: 'resumed' });
    expect(latestNotice(s.hub)?.tone).not.toBe('error');
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
    expect(latestNotice(s.hub)).toMatchObject({
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
    expect(latestNotice(s.hub)).toBeNull();
  });

  it('keeps the paused browser copy (and says so) when cancelling fails after the app took it', async () => {
    s = await setup();
    s.downloads.failing.add('cancel');
    const result = await s.hub.onDownload(downloadItem());
    expect(s.downloads.log).toEqual(['pause:1', 'cancel:1']);
    expect(result).toMatchObject({ kind: 'handed_over', browserCopy: 'left_paused' });
    expect(latestNotice(s.hub)?.message).toContain('still in the browser');
  });

  it.each([
    ['resumed', { state: 'in_progress', paused: false }],
    ['finished', { state: 'complete', paused: false }],
  ] as const)(
    'leaves a browser copy the person %s meanwhile, and warns of two copies',
    async (_label, state) => {
      s = await setup();
      s.app.onRequest = (call) => {
        if (call.method === 'add_download') s.downloads.browserState = { ...state };
      };
      const result = await s.hub.onDownload(downloadItem());
      expect(result).toMatchObject({ kind: 'handed_over', browserCopy: 'kept_running' });
      expect(s.downloads.log).toEqual(['pause:1']);
      expect(latestNotice(s.hub)?.message).toContain('may be downloading twice');
    },
  );

  it('rechecks the app settings after pausing and resumes if they now say no', async () => {
    s = await setup({
      configure: (app, cache) => {
        cache.value = {
          settings: { ...app.settings },
          appClosed: false,
        };
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
    expect(latestNotice(s.hub)?.message).toContain('Allow access');
  });

  it('handles the same download only once at a time', async () => {
    s = await setup({
      configure: (app) => {
        app.addBehavior = 'hang';
      },
    });
    const first = s.hub.onDownload(downloadItem());
    expect(await s.hub.onDownload(downloadItem())).toBeNull();
    await vi.advanceTimersByTimeAsync(5000);
    await first;
  });
});

describe('downloads left paused by a restarted background', () => {
  it('records the download before pausing it and clears it afterwards', async () => {
    s = await setup();
    let savedWhenPaused: InFlightEntry[] = [];
    const pause = s.downloads.pause.bind(s.downloads);
    s.downloads.pause = (id) => {
      savedWhenPaused = [...s.inFlight.entries];
      return pause(id);
    };
    await s.hub.onDownload(downloadItem({ id: 9 }));
    // With the handover id it is offered under, so a restarted background can withdraw it.
    expect(savedWhenPaused).toEqual([{ id: 9, handoverId: 'h1' }]);
    expect(addDownloadCalls(s.app)[0]?.handover_id).toBe('h1');
    expect(s.inFlight.entries).toEqual([]);
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

  it('keeps a notice across a restart until it is dismissed', async () => {
    s = await setup({
      configure: (app) => {
        app.addBehavior = { error: { code: 'internal', message: 'Disk full.' } };
      },
    });
    s.downloads.failing.add('resume');
    await s.hub.onDownload(downloadItem());
    const shown = latestNotice(s.hub);
    expect(shown).toMatchObject({ tone: 'error' });
    s.client.stop();

    s = await setup({ notice: s.notice });
    expect(latestNotice(s.hub)).toEqual(shown);
    expect(s.badges.at(-1)).toBe('!');

    const [popup, background] = channelPair<PopupRequest, BackgroundMessage>();
    s.hub.addPopup(background);
    popup.postMessage({ id: 1, command: { type: 'dismiss_notice', id: shown?.id ?? '' } });
    await flush();
    expect(s.notice.value).toEqual([]);
  });

  it('does not bring back a notice dismissed while it was loading', async () => {
    const notice = new MemoryNotice();
    notice.value = [
      {
        id: 'n1-1',
        downloadId: null,
        untilResumed: false,

        urgent: false,
        tone: 'error',
        message: 'An old notice.',
        at: 1,
      },
    ];
    const release = notice.hold();
    s = await setup({ notice, start: false });
    const [popup, background] = channelPair<PopupRequest, BackgroundMessage>();
    s.hub.addPopup(background);
    popup.postMessage({ id: 1, command: { type: 'dismiss_notice', id: 'n1-1' } });
    await flush();
    release();
    await s.started;
    expect(s.hub.snapshot().notices).toEqual([]);
    expect(notice.value).toEqual([]);
  });

  describe('a stored notice asking to resume a download', () => {
    const stuck: Notice = {
      id: 'd4',
      downloadId: 4,
      untilResumed: true,

      urgent: true,
      tone: 'error',
      message: 'big.zip is paused in the browser.',
      at: 1,
    };

    it('stays away once the browser reports the download running while it loads', async () => {
      const notice = new MemoryNotice();
      notice.value = [stuck];
      const release = notice.hold();
      s = await setup({ notice, start: false });
      s.hub.onDownloadMoved(4);
      release();
      await s.started;
      expect(s.hub.snapshot().notices).toEqual([]);
    });

    it.each([
      ['running', { state: 'in_progress', paused: false }],
      ['finished', { state: 'complete', paused: false }],
      ['gone', null],
    ] as const)('is cleared on load when the download is %s', async (_label, state) => {
      const notice = new MemoryNotice();
      notice.value = [stuck];
      const downloads = new FakeDownloads();
      downloads.states.set(4, state === null ? null : { ...state });
      s = await setup({ notice, downloads });
      expect(s.hub.snapshot().notices).toEqual([]);
      expect(notice.value).toEqual([]);
    });

    it('stays on load while the download is still paused', async () => {
      const notice = new MemoryNotice();
      notice.value = [stuck];
      s = await setup({ notice });
      expect(s.hub.snapshot().notices).toEqual([stuck]);
    });
  });

  it('keeps a notice raised while the stored one was loading', async () => {
    const notice = new MemoryNotice();
    notice.value = [
      {
        id: 'n1-1',
        downloadId: null,
        untilResumed: false,

        urgent: false,
        tone: 'error',
        message: 'An old notice.',
        at: 1,
      },
    ];
    const release = notice.hold();
    s = await setup({ notice, start: false });
    await s.hub.onMenuClick({ linkUrl: 'ftp://example.com/a.zip' }, undefined);
    release();
    await s.started;
    expect(s.hub.snapshot().notices.map((n) => n.message)).toEqual([
      MENU_NOT_WEB,
      'An old notice.',
    ]);
  });

  describe('after a restart, lets withdraw_download decide', () => {
    async function recover(
      withdraw: FakeApp['withdrawBehavior'],
      browserState: DownloadState | null = { state: 'in_progress', paused: true },
    ): Promise<{ downloads: FakeDownloads; inFlight: MemoryInFlight }> {
      const inFlight = new MemoryInFlight();
      inFlight.entries = [{ id: 5, handoverId: 'h-before' }];
      const downloads = new FakeDownloads();
      downloads.browserState = browserState;
      s = await setup({
        inFlight,
        downloads,
        start: false,
        configure: (app) => {
          app.withdrawBehavior = withdraw;
        },
      });
      await vi.advanceTimersByTimeAsync(20_000);
      await s.started;
      return { downloads, inFlight };
    }

    it('cancels the browser copy when the app already has it', async () => {
      const { downloads, inFlight } = await recover('already_started');
      expect(s.app.calls('withdraw_download')).toEqual([
        { method: 'withdraw_download', params: { handover_id: 'h-before' } },
      ]);
      expect(downloads.log).toEqual(['cancel:5', 'erase:5']);
      expect(latestNotice(s.hub)).toBeNull();
      expect(inFlight.entries).toEqual([]);
    });

    it('leaves a browser copy the person resumed while the withdraw was waiting', async () => {
      const inFlight = new MemoryInFlight();
      inFlight.entries = [{ id: 5, handoverId: 'h-before' }];
      const downloads = new FakeDownloads();
      downloads.browserState = { state: 'in_progress', paused: true };
      s = await setup({
        inFlight,
        downloads,
        configure: (app) => {
          app.withdrawBehavior = 'already_started';
          app.onRequest = (call) => {
            if (call.method === 'withdraw_download') {
              downloads.browserState = { state: 'in_progress', paused: false };
            }
          };
        },
      });
      expect(downloads.log).toEqual([]);
      expect(latestNotice(s.hub)?.message).toContain('may be downloading twice');
    });

    it.each(['withdrawn', 'not_found'] as const)(
      'resumes the browser copy without a warning when the app says "%s"',
      async (outcome) => {
        const { downloads, inFlight } = await recover(outcome);
        expect(downloads.log).toEqual(['resume:5']);
        expect(latestNotice(s.hub)).toBeNull();
        expect(inFlight.entries).toEqual([]);
      },
    );

    it.each([
      ['no answer comes', 'hang'],
      ['the answer cannot be read', 'unreadable'],
      ['the app fails', { error: { code: 'internal', message: 'Database locked.' } }],
    ] as const)(
      'resumes the browser copy and warns it may download twice when %s',
      async (_label, behavior) => {
        const { downloads } = await recover(behavior, {
          state: 'in_progress',
          paused: true,
          filename: '/home/me/Downloads/big.zip',
        });
        expect(downloads.log).toEqual(['resume:5']);
        expect(latestNotice(s.hub)).toMatchObject({ tone: 'warning' });
        expect(latestNotice(s.hub)?.message).toMatch(/^big\.zip may be downloading twice/);
      },
    );

    it('still asks the app, and resumes the copy, when the browser cannot say how it stands', async () => {
      const inFlight = new MemoryInFlight();
      inFlight.entries = [{ id: 5, handoverId: 'h-before' }];
      const downloads = new FakeDownloads();
      downloads.failing.add('state');
      s = await setup({ inFlight, downloads });
      expect(s.app.calls('withdraw_download')).toHaveLength(1);
      expect(downloads.log).toEqual(['resume:5']);
      expect(inFlight.entries).toEqual([]);
    });

    it('says the copy may be paused when resume fails and the browser cannot say', async () => {
      const inFlight = new MemoryInFlight();
      inFlight.entries = [{ id: 5, handoverId: 'h-before' }];
      const downloads = new FakeDownloads();
      downloads.failing.add('state');
      downloads.failing.add('resume');
      s = await setup({ inFlight, downloads });
      expect(latestNotice(s.hub)).toMatchObject({ tone: 'error', downloadId: 5 });
      expect(latestNotice(s.hub)?.message).toContain('may be paused in the browser');
    });

    it('says Wirecatch may have a copy that will not resume when the withdraw gets no answer', async () => {
      const inFlight = new MemoryInFlight();
      inFlight.entries = [{ id: 5, handoverId: 'h5' }];
      const downloads = new FakeDownloads();
      downloads.failingCalls.add('resume:5');
      downloads.states.set(5, { state: 'in_progress', paused: true });
      s = await setup({
        inFlight,
        downloads,
        start: false,
        configure: (app) => {
          app.withdrawFor.set('h5', 'hang');
        },
      });
      await vi.advanceTimersByTimeAsync(20_000);
      await s.started;
      expect(latestNotice(s.hub)).toMatchObject({ tone: 'error', urgent: true, downloadId: 5 });
      expect(latestNotice(s.hub)?.message).toContain('Wirecatch may have taken it');
    });

    it('raises no error for a copy the person resumed while the withdraw was waiting', async () => {
      const inFlight = new MemoryInFlight();
      inFlight.entries = [{ id: 5, handoverId: 'h-before' }];
      const downloads = new FakeDownloads();
      downloads.failing.add('resume');
      s = await setup({
        inFlight,
        downloads,
        configure: (app) => {
          app.onRequest = (call) => {
            if (call.method === 'withdraw_download') {
              downloads.browserState = { state: 'in_progress', paused: false };
            }
          };
        },
      });
      expect(latestNotice(s.hub)).toBeNull();
    });

    it('says how to recover a download it cannot resume', async () => {
      const inFlight = new MemoryInFlight();
      inFlight.entries = [{ id: 5, handoverId: 'h-before' }];
      const downloads = new FakeDownloads();
      downloads.browserState = { state: 'in_progress', paused: true };
      downloads.failing.add('resume');
      s = await setup({ inFlight, downloads });
      expect(latestNotice(s.hub)).toMatchObject({
        tone: 'error',
        message:
          "A download is paused in the browser. Open the browser's downloads page and click Resume.",
      });
    });

    it.each([
      ['resumed', { state: 'in_progress', paused: false }],
      ['finished', { state: 'complete', paused: false }],
    ] as const)(
      'asks the app about a download the person %s, and settles it',
      async (_l, state) => {
        const { downloads, inFlight } = await recover('withdrawn', { ...state });
        expect(s.app.calls('withdraw_download')).toHaveLength(1);
        expect(downloads.log).toEqual([]);
        expect(latestNotice(s.hub)).toBeNull();
        expect(inFlight.entries).toEqual([]);
      },
    );

    it('warns, without cancelling it, when the app has a download the person resumed', async () => {
      const { downloads } = await recover('already_started', {
        state: 'in_progress',
        paused: false,
      });
      expect(downloads.log).toEqual([]);
      expect(latestNotice(s.hub)?.message).toContain('may be downloading twice');
    });

    it('warns when the app cannot say whether it has a download the person resumed', async () => {
      const { downloads } = await recover('hang', { state: 'in_progress', paused: false });
      expect(downloads.log).toEqual([]);
      expect(latestNotice(s.hub)?.message).toContain('may be downloading twice');
    });

    it('keeps a notice for each of two downloads', async () => {
      const inFlight = new MemoryInFlight();
      inFlight.entries = [
        { id: 5, handoverId: 'h5' },
        { id: 6, handoverId: 'h6' },
      ];
      const downloads = new FakeDownloads();
      downloads.failingCalls.add('resume:5');
      downloads.states.set(6, { state: 'in_progress', paused: false });
      s = await setup({
        inFlight,
        downloads,
        start: false,
        configure: (app) => {
          app.withdrawFor.set('h6', 'hang');
        },
      });
      await vi.advanceTimersByTimeAsync(20_000);
      await s.started;
      expect(s.hub.snapshot().notices).toMatchObject([
        { downloadId: 6, tone: 'warning' },
        { downloadId: 5, tone: 'error', untilResumed: true },
      ]);
    });

    it('waits for an app it has to start for as long as hello may take', async () => {
      const inFlight = new MemoryInFlight();
      inFlight.entries = [{ id: 5, handoverId: 'h5' }];
      const downloads = new FakeDownloads();
      s = await setup({
        inFlight,
        downloads,
        start: false,
        configure: (app, cache) => {
          app.running = false;
          app.startDelayMs = 9500;
          app.withdrawBehavior = 'already_started';
          cache.value = { settings: DEFAULT_SETTINGS, appClosed: true };
        },
      });
      await vi.advanceTimersByTimeAsync(20_000);
      await s.started;
      expect(downloads.log).toEqual(['cancel:5', 'erase:5']);
      expect(latestNotice(s.hub)).toBeNull();
    });

    it('keeps the entries not yet settled saved, so another restart still has them', async () => {
      const inFlight = new MemoryInFlight();
      inFlight.entries = [
        { id: 5, handoverId: 'h5' },
        { id: 6, handoverId: 'h6' },
      ];
      s = await setup({ inFlight });
      expect(inFlight.saves[0]).toEqual([{ id: 6, handoverId: 'h6' }]);
      expect(inFlight.entries).toEqual([]);
    });

    it('leaves a download being handed over again to that handover', async () => {
      const inFlight = new MemoryInFlight();
      inFlight.entries = [{ id: 5, handoverId: 'h-before' }];
      s = await setup({ inFlight, start: false });
      const download = s.hub.onDownload(downloadItem({ id: 5 }));
      await s.started;
      await download;
      expect(s.app.calls('withdraw_download')).toHaveLength(0);
    });

    it.each([
      ['cancelled', { state: 'interrupted', paused: false }],
      ['taken off the list', null],
    ] as const)('does not ask the app about a download the person %s', async (_l, state) => {
      const { downloads, inFlight } = await recover(
        'withdrawn',
        state === null ? null : { ...state },
      );
      expect(s.app.calls('withdraw_download')).toHaveLength(0);
      expect(downloads.log).toEqual([]);
      expect(inFlight.entries).toEqual([]);
    });
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
      // Not in a frame: the page itself is the document the link is in.
      frame_url: 'https://example.com/page',
      cookies: 'session=abc',
      source: 'context_menu',
      queue: 1,
    });
    expect(s.cookieCalls).toEqual([
      ['https://example.com/file.iso', { id: 'firefox-container-2', private: false }],
    ]);
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

  it('names the frame the link is in, which chose it, apart from the page', async () => {
    s = await setup();
    await s.hub.onMenuClick(
      {
        linkUrl: 'http://192.168.1.20/a.zip',
        pageUrl: 'http://portal/home',
        frameUrl: 'https://widget.example.net/embed',
      },
      undefined,
    );
    expect(addDownloadCalls(s.app)[0]).toMatchObject({
      page_url: 'http://portal/home',
      frame_url: 'https://widget.example.net/embed',
      // From https to http the browser sends no referrer, so it cannot stand for the frame.
      referrer: null,
    });
  });

  it('uses the default cookie store in Chromium (no cookieStoreId)', async () => {
    s = await setup();
    await s.hub.onMenuClick(
      { linkUrl: 'https://example.com/a.zip' },
      { incognito: false, cookieStoreId: null },
    );
    expect(s.cookieCalls).toEqual([['https://example.com/a.zip', { id: null, private: false }]]);
  });

  it("takes a private window's link with that window's cookies (D82)", async () => {
    s = await setup();
    const result = await s.hub.onMenuClick(
      { linkUrl: 'https://example.com/a.zip' },
      { incognito: true, cookieStoreId: null },
    );
    expect(result).toBe('handed_over');
    expect(s.cookieCalls).toEqual([['https://example.com/a.zip', { id: null, private: true }]]);
  });

  it('asks the app before taking a private window while the settings are not known yet', async () => {
    s = await setup({ start: false, configure: privateWindowsOff });
    const result = await s.hub.onMenuClick(
      { linkUrl: 'https://example.com/a.zip' },
      { incognito: true, cookieStoreId: null },
    );
    expect(result).toBe('private_window');
    expect(s.app.calls('add_download')).toHaveLength(0);
  });

  it('leaves private windows to the browser when they are turned off', async () => {
    s = await setup({ configure: privateWindowsOff });
    const result = await s.hub.onMenuClick(
      { linkUrl: 'https://example.com/a.zip' },
      { incognito: true, cookieStoreId: null },
    );
    expect(result).toBe('private_window');
    expect(s.app.calls('add_download')).toHaveLength(0);
    expect(s.downloads.log).toEqual([]);
    expect(latestNotice(s.hub)?.message).toContain('private windows');
  });

  it('lets the browser download it without site access, after the earlier checks', async () => {
    s = await setup({ siteAccess: false, configure: privateWindowsOff });
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
    expect(latestNotice(s.hub)?.message).toContain('Allow access');
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
    expect(latestNotice(s.hub)?.message).toContain('the browser is downloading it instead');
  });

  it('withdraws after a timeout: "already_started" means the app has it', async () => {
    s = await setup({
      configure: (app) => {
        app.addBehavior = 'hang';
        app.withdrawBehavior = 'already_started';
      },
    });
    const pending = s.hub.onMenuClick({ linkUrl: 'https://example.com/file.iso' }, undefined);
    await vi.advanceTimersByTimeAsync(5000);
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
    await vi.advanceTimersByTimeAsync(5000);
    expect(await pending).toBe('browser');
    expect(s.downloads.log).toEqual(['start:https://example.com/file.iso']);
  });

  it('warns of two copies when the app cannot say whether it took the link', async () => {
    s = await setup({
      configure: (app) => {
        app.addBehavior = 'hang';
        app.withdrawBehavior = 'hang';
      },
    });
    const pending = s.hub.onMenuClick({ linkUrl: 'https://example.com/file.iso' }, undefined);
    await vi.advanceTimersByTimeAsync(8000);
    expect(await pending).toBe('browser');
    expect(latestNotice(s.hub)?.message).toBe(
      'file.iso may be downloading twice: Wirecatch may have taken it, and the browser is downloading it too. If Wirecatch has it, cancel one of them.',
    );
  });

  it('sends a program the person chose to Wirecatch like any other link (D60)', async () => {
    s = await setup();
    const result = await s.hub.onMenuClick(
      { linkUrl: 'https://example.com/get/Setup.EXE' },
      undefined,
    );
    expect(result).toBe('handed_over');
    expect(s.downloads.log).toEqual([]);
    const call = s.app.calls('add_download').at(-1);
    expect(call?.method === 'add_download' && call.params.url).toBe(
      'https://example.com/get/Setup.EXE',
    );
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
    expect(latestNotice(s.hub)).toMatchObject({ tone: 'error' });
  });
});
