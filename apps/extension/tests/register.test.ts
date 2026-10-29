/**
 * Chromium waits for `suggest` before it continues a download, so it must be
 * called exactly once on every path or the download hangs.
 */
import { afterEach, describe, expect, it, vi } from 'vitest';
import { PATIENCE_MS } from '@wirecatch/ui/testing/jsdom';
import type { DownloadItemLike } from '../src/lib/capture/candidate';
import {
  registerCapture,
  type CaptureHandler,
  type DownloadDelta,
  type DownloadEvents,
} from '../src/lib/capture/register';
import { diagnostics } from '../src/lib/diag/log';
import { Hub } from '../src/lib/hub/hub';
import { NativeClient } from '../src/lib/native/client';
import { FakeApp, FakeDownloads, MemoryCache, downloadItem, flush } from './fakes';

type Determining = (item: DownloadItemLike, suggest: () => void) => boolean | undefined;

function chromiumEvents(): {
  events: DownloadEvents;
  change: (delta: DownloadDelta) => void;
  erase: (downloadId: number) => void;
  fire: (item: DownloadItemLike) => {
    suggest: ReturnType<typeof vi.fn>;
    returned: boolean | undefined;
  };
} {
  let listener: Determining | null = null;
  let changed: ((delta: DownloadDelta) => void) | null = null;
  let erased: ((downloadId: number) => void) | null = null;
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
    onChanged: {
      addListener: (l) => {
        changed = l;
      },
    },
    onErased: {
      addListener: (l) => {
        erased = l;
      },
    },
  };
  return {
    events,
    change: (delta) => {
      if (changed === null) throw new Error('not registered');
      changed(delta);
    },
    erase: (downloadId) => {
      if (erased === null) throw new Error('not registered');
      erased(downloadId);
    },
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

describe('download changes', () => {
  it.each<[string, DownloadDelta, boolean]>([
    ['resumed', { id: 3, paused: { current: false } }, true],
    ['finished', { id: 3, state: { current: 'complete' } }, true],
    ['cancelled', { id: 3, state: { current: 'interrupted' } }, true],
    ['paused', { id: 3, paused: { current: true } }, false],
    ['renamed', { id: 3 }, false],
  ])('tells the hub when a download is %s: %s', (_label, delta, moved) => {
    const { events, change } = chromiumEvents();
    const onDownloadMoved = vi.fn();
    registerCapture(events, {
      onDownload: () => Promise.resolve(null),
      onDownloadMoved,
    });
    change(delta);
    expect(onDownloadMoved).toHaveBeenCalledTimes(moved ? 1 : 0);
  });

  it('tells the hub when a download is erased from the list', () => {
    const { events, erase } = chromiumEvents();
    const onDownloadMoved = vi.fn();
    registerCapture(events, { onDownload: () => Promise.resolve(null), onDownloadMoved });
    erase(3);
    expect(onDownloadMoved).toHaveBeenCalledWith(3);
  });
});

describe('registerCapture on Chromium', () => {
  it('uses onDeterminingFilename and tells Chromium to wait for suggest', async () => {
    const { events, fire } = chromiumEvents();
    expect(
      registerCapture(events, {
        onDownload: () => Promise.resolve(null),
        onDownloadMoved: () => undefined,
      }),
    ).toBe('determining_filename');
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
    await vi.waitFor(() => expect(suggest).toHaveBeenCalledTimes(1), PATIENCE_MS);
    await flush();
    expect(suggest).toHaveBeenCalledTimes(1);
  });

  it('calls suggest only after the handover finished (so the download waits)', async () => {
    let finish: () => void = () => undefined;
    const handler: CaptureHandler = {
      onDownloadMoved: () => undefined,
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
    registerCapture(events, {
      onDownload: () => Promise.reject(new Error('boom')),
      onDownloadMoved: () => undefined,
    });
    diagnostics.clear();
    const { suggest } = fire(downloadItem());
    await flush();
    expect(suggest).toHaveBeenCalledTimes(1);
    expect(diagnostics.entries()).toEqual([
      expect.objectContaining({ level: 'error', scope: 'capture', detail: 'Error: boom' }),
    ]);
  });

  it('calls suggest once when the handler throws before returning a promise', async () => {
    const { events, fire } = chromiumEvents();
    registerCapture(events, {
      onDownloadMoved: () => undefined,
      onDownload: () => {
        throw new Error('boom');
      },
    });
    diagnostics.clear();
    const { suggest, returned } = fire(downloadItem());
    expect(returned).toBe(true);
    expect(suggest).toHaveBeenCalledTimes(1);
    await flush();
    expect(suggest).toHaveBeenCalledTimes(1);
    expect(diagnostics.entries()).toEqual([
      expect.objectContaining({ level: 'error', scope: 'capture', detail: 'Error: boom' }),
    ]);
  });

  it('survives suggest throwing (the download was cancelled meanwhile)', async () => {
    const { events, fire } = chromiumEvents();
    registerCapture(events, {
      onDownload: () => Promise.resolve(null),
      onDownloadMoved: () => undefined,
    });
    const { suggest } = fire(downloadItem());
    suggest.mockImplementation(() => {
      throw new Error('Download must be in progress');
    });
    await flush();
    expect(suggest).toHaveBeenCalledTimes(1);
  });
});

describe('registerCapture on Firefox', () => {
  it('uses onCreated and logs handler failures', async () => {
    let listener: ((item: DownloadItemLike) => void) | null = null;
    const events: DownloadEvents = {
      onChanged: { addListener: () => undefined },
      onErased: { addListener: () => undefined },
      onCreated: {
        addListener: (l) => {
          listener = l;
        },
      },
    };
    const onDownload = vi.fn(() => Promise.reject(new Error('boom')));
    expect(registerCapture(events, { onDownload, onDownloadMoved: () => undefined })).toBe(
      'created',
    );
    const fire = listener as ((item: DownloadItemLike) => void) | null;
    expect(fire).not.toBeNull();
    diagnostics.clear();
    fire?.(downloadItem());
    await flush();
    expect(onDownload).toHaveBeenCalledTimes(1);
    expect(diagnostics.entries()).toEqual([
      expect.objectContaining({ level: 'error', scope: 'capture', detail: 'Error: boom' }),
    ]);
  });
});
