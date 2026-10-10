/**
 * The most important rule: no download is ever lost. These tests run the real hub,
 * handover and native client against a fake browser downloads API and a fake port.
 */
import type { NewDownload } from '@wirecatch/protocol';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { Hub } from '../src/lib/hub';
import { NativeClient } from '../src/lib/native/client';
import { FakeApp, FakeDownloads, MemoryCache, downloadItem, flush } from './fakes';

interface Setup {
  app: FakeApp;
  downloads: FakeDownloads;
  client: NativeClient;
  hub: Hub;
  cache: MemoryCache;
}

async function setup(
  configure: (app: FakeApp, cache: MemoryCache) => void = () => undefined,
): Promise<Setup> {
  const app = new FakeApp();
  const cache = new MemoryCache();
  configure(app, cache);
  const downloads = new FakeDownloads();
  const client = new NativeClient({
    transport: app,
    hello: () =>
      Promise.resolve({ client: 'extension', client_version: '0.1.0', protocol_version: 1, browser: null }),
    backoffMs: [60_000],
  });
  const hub = new Hub({
    client,
    cache,
    downloads,
    cookieHeader: (url) => Promise.resolve(url.includes('example.com') ? 'session=abc' : null),
    userAgent: 'TestBrowser/1.0',
    connectWaitMs: 3_000,
    requestTimeoutMs: 5_000,
  });
  await hub.start();
  await flush();
  return { app, downloads, client, hub, cache };
}

function addDownloadCalls(app: FakeApp): NewDownload[] {
  return app.calls('add_download').map((c) => (c.method === 'add_download' ? c.params : (null as never)));
}

describe('handover', () => {
  let s: Setup;

  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    s.client.stop();
    vi.useRealTimers();
  });

  it('pauses, hands over everything the app needs, then cancels and erases only after the app confirms', async () => {
    s = await setup();
    let logWhenAppAsked: string[] = [];
    s.app.onRequest = (call) => {
      if (call.method === 'add_download') logWhenAppAsked = [...s.downloads.log];
    };

    const result = await s.hub.onDownload(downloadItem({ id: 42 }));

    expect(logWhenAppAsked).toEqual(['pause:42']);
    expect(s.downloads.log).toEqual(['pause:42', 'cancel:42', 'erase:42']);
    expect(result).toEqual({ kind: 'handed_over', action: 'started', browserCopyRemoved: true });
    expect(addDownloadCalls(s.app)).toEqual([
      {
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

  it.each(['queued', 'awaiting_confirmation'] as const)('treats "%s" as confirmed', async (action) => {
    s = await setup((app) => {
      app.addBehavior = action;
    });
    const result = await s.hub.onDownload(downloadItem());
    expect(result).toMatchObject({ kind: 'handed_over', action });
    expect(s.downloads.log).toEqual(['pause:1', 'cancel:1', 'erase:1']);
  });

  it('resumes the browser download when the app answers with an error', async () => {
    s = await setup((app) => {
      app.addBehavior = { error: { code: 'invalid_url', message: 'That address cannot be downloaded.' } };
    });
    const result = await s.hub.onDownload(downloadItem());
    expect(s.downloads.log).toEqual(['pause:1', 'resume:1']);
    expect(result).toEqual({
      kind: 'kept',
      reason: 'app_refused',
      detail: 'That address cannot be downloaded.',
      resumed: true,
    });
    expect(s.hub.snapshot().notice?.message).toContain('stayed in the browser');
  });

  it('resumes the browser download when the app does not answer in time', async () => {
    s = await setup((app) => {
      app.addBehavior = 'hang';
    });
    const pending = s.hub.onDownload(downloadItem());
    await vi.advanceTimersByTimeAsync(4_999);
    expect(s.downloads.log).toEqual(['pause:1']);
    await vi.advanceTimersByTimeAsync(1);
    const result = await pending;
    expect(s.downloads.log).toEqual(['pause:1', 'resume:1']);
    expect(result).toMatchObject({ kind: 'kept', reason: 'app_refused', resumed: true });
  });

  it('resumes the browser download when the connection breaks mid-handover', async () => {
    s = await setup((app) => {
      app.addBehavior = 'disconnect';
    });
    const result = await s.hub.onDownload(downloadItem());
    expect(s.downloads.log).toEqual(['pause:1', 'resume:1']);
    expect(result).toMatchObject({ kind: 'kept', reason: 'app_refused', resumed: true });
  });

  it('resumes the browser download on an answer it does not understand', async () => {
    s = await setup((app) => {
      app.addBehavior = 'unexpected_action';
    });
    const result = await s.hub.onDownload(downloadItem());
    expect(s.downloads.log).toEqual(['pause:1', 'resume:1']);
    expect(result).toMatchObject({ kind: 'kept', reason: 'app_refused' });
  });

  it('resumes the browser download when the app cannot be reached at all', async () => {
    s = await setup((app) => {
      app.hostMissing = 'Specified native messaging host not found.';
    });
    const result = await s.hub.onDownload(downloadItem());
    expect(s.downloads.log).toEqual(['pause:1', 'resume:1']);
    expect(result).toMatchObject({ kind: 'kept', reason: 'app_unreachable', resumed: true });
    expect(s.app.calls('add_download')).toHaveLength(0);
  });

  it('resumes the browser download when the bridge cannot start the app', async () => {
    s = await setup((app) => {
      app.helloError = { code: 'app_unavailable', message: 'Could not start Wirecatch.' };
    });
    const result = await s.hub.onDownload(downloadItem());
    expect(s.downloads.log).toEqual(['pause:1', 'resume:1']);
    expect(result).toMatchObject({ kind: 'kept', reason: 'app_unreachable' });
  });

  it('waits for a connection that is still starting, then hands over', async () => {
    s = await setup((app) => {
      app.hostMissing = 'Specified native messaging host not found.';
    });
    s.app.hostMissing = null; // The app becomes available (for example, the bridge got installed).
    const result = await s.hub.onDownload(downloadItem());
    expect(result).toMatchObject({ kind: 'handed_over' });
    expect(s.downloads.log).toEqual(['pause:1', 'cancel:1', 'erase:1']);
  });

  it('leaves the download untouched when the browser will not pause it', async () => {
    s = await setup();
    s.downloads.failing.add('pause');
    const result = await s.hub.onDownload(downloadItem());
    expect(s.downloads.log).toEqual(['pause:1']);
    expect(result).toMatchObject({ kind: 'kept', reason: 'pause_failed' });
    expect(s.app.calls('add_download')).toHaveLength(0);
  });

  it('says how to recover when the browser will not resume', async () => {
    s = await setup((app) => {
      app.addBehavior = { error: { code: 'internal', message: 'Disk full.' } };
    });
    s.downloads.failing.add('resume');
    const result = await s.hub.onDownload(downloadItem());
    expect(result).toMatchObject({ kind: 'kept', resumed: false });
    expect(s.hub.snapshot().notice).toMatchObject({ tone: 'error' });
    expect(s.hub.snapshot().notice?.message).toContain('Resume');
  });

  it('keeps the paused browser copy (and says so) when cancelling fails after the app took it', async () => {
    s = await setup();
    s.downloads.failing.add('cancel');
    const result = await s.hub.onDownload(downloadItem());
    expect(s.downloads.log).toEqual(['pause:1', 'cancel:1']);
    expect(result).toEqual({ kind: 'handed_over', action: 'started', browserCopyRemoved: false });
    expect(s.hub.snapshot().notice?.message).toContain('still in the browser');
  });

  it('rechecks the app settings after pausing and resumes if they now say no', async () => {
    s = await setup((app, cache) => {
      cache.value = { settings: { ...app.settings }, queues: [], savedAt: 1 };
      app.settings = { ...app.settings, capture_enabled: false };
      app.hostMissing = 'Specified native messaging host not found.';
    });
    s.app.hostMissing = null;
    const result = await s.hub.onDownload(downloadItem());
    expect(s.downloads.log).toEqual(['pause:1', 'resume:1']);
    expect(result).toMatchObject({ kind: 'kept', reason: 'capture_off' });
  });

  it('does not touch downloads the settings exclude', async () => {
    s = await setup((app) => {
      app.settings = { ...app.settings, ignored_sites: ['example.com'] };
    });
    expect(await s.hub.onDownload(downloadItem())).toBeNull();
    expect(await s.hub.onDownload(downloadItem({ url: 'blob:https://x.org/1', finalUrl: '' }))).toBeNull();
    expect(s.downloads.log).toEqual([]);
  });

  it('handles the same download only once at a time', async () => {
    s = await setup((app) => {
      app.addBehavior = 'hang';
    });
    const first = s.hub.onDownload(downloadItem());
    expect(await s.hub.onDownload(downloadItem())).toBeNull();
    await vi.advanceTimersByTimeAsync(5_000);
    await first;
  });
});

describe('context menu', () => {
  let s: Setup;

  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    s.client.stop();
    vi.useRealTimers();
  });

  it('sends the link with source context_menu', async () => {
    s = await setup();
    const result = await s.hub.onMenuClick({
      linkUrl: 'https://example.com/file.iso',
      pageUrl: 'https://example.com/page',
    });
    expect(result).toBe('handed_over');
    expect(addDownloadCalls(s.app)[0]).toMatchObject({
      url: 'https://example.com/file.iso',
      referrer: 'https://example.com/page',
      page_url: 'https://example.com/page',
      cookies: 'session=abc',
      source: 'context_menu',
      queue: 1,
    });
    expect(s.downloads.log).toEqual([]);
  });

  it('uses the media address when there is no link', async () => {
    s = await setup();
    await s.hub.onMenuClick({ srcUrl: 'https://media.example.org/v.mp4', pageUrl: 'https://example.org/' });
    expect(addDownloadCalls(s.app)[0]?.url).toBe('https://media.example.org/v.mp4');
  });

  it('lets the browser download it when the app does not take it', async () => {
    s = await setup((app) => {
      app.addBehavior = { error: { code: 'internal', message: 'Something broke.' } };
    });
    const result = await s.hub.onMenuClick({ linkUrl: 'https://example.com/file.iso' });
    expect(result).toBe('browser');
    expect(s.downloads.log).toEqual(['start:https://example.com/file.iso']);
    expect(s.hub.snapshot().notice?.message).toContain('the browser is downloading it instead');
  });

  it('lets the browser handle addresses only it can open', async () => {
    s = await setup();
    const result = await s.hub.onMenuClick({ srcUrl: 'data:image/png;base64,AAAA' });
    expect(result).toBe('browser');
    expect(s.app.calls('add_download')).toHaveLength(0);
  });

  it('reports when neither the app nor the browser could download it', async () => {
    s = await setup((app) => {
      app.hostMissing = 'Specified native messaging host not found.';
    });
    s.downloads.failing.add('start');
    const result = await s.hub.onMenuClick({ srcUrl: 'blob:https://example.com/123' });
    expect(result).toBe('failed');
    expect(s.hub.snapshot().notice).toMatchObject({ tone: 'error' });
  });
});
