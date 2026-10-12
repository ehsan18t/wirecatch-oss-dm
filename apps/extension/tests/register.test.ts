/**
 * Chromium waits for `suggest` before it continues a download, so it must be
 * called exactly once on every path or the download hangs.
 */
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { DownloadItemLike } from '../src/lib/capture/candidate';
import {
  registerCapture,
  type CaptureHandler,
  type DownloadEvents,
} from '../src/lib/capture/register';
import { Hub } from '../src/lib/hub/hub';
import { NativeClient } from '../src/lib/native/client';
import { FakeApp, FakeDownloads, MemoryCache, downloadItem, flush } from './fakes';

type Determining = (item: DownloadItemLike, suggest: () => void) => boolean | undefined;

function chromiumEvents(): {
  events: DownloadEvents;
  fire: (item: DownloadItemLike) => {
    suggest: ReturnType<typeof vi.fn>;
    returned: boolean | undefined;
  };
} {
  let listener: Determining | null = null;
  const events: DownloadEvents = {
    onDeterminingFilename: {
      addListener: (l) => {
        listener = l;
      },
    },
    onCreated: {
      addListener: () => {
        throw new Error('Chromium must not use onCreated');
      },
    },
  };
  return {
    events,
    fire: (item) => {
      const suggest = vi.fn();
      if (listener === null) throw new Error('not registered');
      const returned = listener(item, suggest);
      return { suggest, returned };
    },
  };
}

const stopped: NativeClient[] = [];
afterEach(() => {
  for (const c of stopped.splice(0)) c.stop();
  vi.useRealTimers();
});

async function realHub(
  configure: (app: FakeApp, downloads: FakeDownloads) => void = () => undefined,
): Promise<Hub> {
  const app = new FakeApp();
  const downloads = new FakeDownloads();
  configure(app, downloads);
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
  stopped.push(client);
  const hub = new Hub({
    client,
    cache: new MemoryCache(),
    downloads,
    cookieHeader: () => Promise.resolve(null),
    hasSiteAccess: () => Promise.resolve(true),
    userAgent: null,
  });
  await hub.start();
  await flush();
  return hub;
}

describe('registerCapture on Chromium', () => {
  it('uses onDeterminingFilename and tells Chromium to wait for suggest', async () => {
    const { events, fire } = chromiumEvents();
    expect(registerCapture(events, { onDownload: () => Promise.resolve(null) })).toBe(
      'determining_filename',
    );
    const { suggest, returned } = fire(downloadItem());
    expect(returned).toBe(true);
    await flush();
    expect(suggest).toHaveBeenCalledTimes(1);
  });

  it.each<[string, (app: FakeApp, downloads: FakeDownloads) => void]>([
    ['handed over', () => undefined],
    [
      'kept',
      (app) => {
        app.addBehavior = { error: { code: 'internal', message: 'No.' } };
      },
    ],
    [
      'skipped',
      (app) => {
        app.settings = { ...app.settings, capture_enabled: false };
      },
    ],
  ])('calls suggest exactly once when the download is %s (real hub)', async (_label, configure) => {
    const hub = await realHub(configure);
    const { events, fire } = chromiumEvents();
    registerCapture(events, hub);
    const { suggest } = fire(downloadItem());
    await vi.waitFor(() => expect(suggest).toHaveBeenCalledTimes(1));
    await flush();
    expect(suggest).toHaveBeenCalledTimes(1);
  });

  it('calls suggest only after the handover finished (so the download waits)', async () => {
    let finish: () => void = () => undefined;
    const handler: CaptureHandler = {
      onDownload: () =>
        new Promise((resolve) => {
          finish = () => resolve(null);
        }),
    };
    const { events, fire } = chromiumEvents();
    registerCapture(events, handler);
    const { suggest } = fire(downloadItem());
    await flush();
    expect(suggest).not.toHaveBeenCalled();
    finish();
    await flush();
    expect(suggest).toHaveBeenCalledTimes(1);
  });

  it('calls suggest once for an in-flight duplicate and once for the original', async () => {
    const hub = await realHub((app) => {
      app.addBehavior = 'hang';
    });
    vi.useFakeTimers();
    const { events, fire } = chromiumEvents();
    registerCapture(events, hub);
    const first = fire(downloadItem());
    const second = fire(downloadItem());
    await vi.advanceTimersByTimeAsync(0);
    expect(second.suggest).toHaveBeenCalledTimes(1);
    expect(first.suggest).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(15_000);
    expect(first.suggest).toHaveBeenCalledTimes(1);
  });

  it('calls suggest once when the handler rejects', async () => {
    const { events, fire } = chromiumEvents();
    registerCapture(events, { onDownload: () => Promise.reject(new Error('boom')) });
    const { suggest } = fire(downloadItem());
    await flush();
    expect(suggest).toHaveBeenCalledTimes(1);
  });

  it('calls suggest once when the handler throws before returning a promise', async () => {
    const { events, fire } = chromiumEvents();
    registerCapture(events, {
      onDownload: () => {
        throw new Error('boom');
      },
    });
    const { suggest, returned } = fire(downloadItem());
    expect(returned).toBe(true);
    expect(suggest).toHaveBeenCalledTimes(1);
    await flush();
    expect(suggest).toHaveBeenCalledTimes(1);
  });

  it('survives suggest throwing (the download was cancelled meanwhile)', async () => {
    const { events, fire } = chromiumEvents();
    registerCapture(events, { onDownload: () => Promise.resolve(null) });
    const { suggest } = fire(downloadItem());
    suggest.mockImplementation(() => {
      throw new Error('Download must be in progress');
    });
    await flush();
    expect(suggest).toHaveBeenCalledTimes(1);
  });
});

describe('registerCapture on Firefox', () => {
  it('uses onCreated and swallows handler failures', async () => {
    let listener: ((item: DownloadItemLike) => void) | null = null;
    const events: DownloadEvents = {
      onCreated: {
        addListener: (l) => {
          listener = l;
        },
      },
    };
    const onDownload = vi.fn(() => Promise.reject(new Error('boom')));
    expect(registerCapture(events, { onDownload })).toBe('created');
    const fire = listener as ((item: DownloadItemLike) => void) | null;
    expect(fire).not.toBeNull();
    fire?.(downloadItem());
    await flush();
    expect(onDownload).toHaveBeenCalledTimes(1);
  });
});
