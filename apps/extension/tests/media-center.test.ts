/**
 * The background's media side: what the media button may show and send, the
 * popup's list, and the handover to the app (real hub and native client, fake app).
 */
import type { NewDownload } from '@wirecatch/protocol';
import { afterEach, describe, expect, it } from 'vitest';
import type { CookieContext } from '../src/lib/cookies';
import { Hub, MEDIA_UNREACHABLE } from '../src/lib/hub';
import {
  MediaCenter,
  PRIVATE_WINDOW,
  type ContentSender,
  type MediaView,
  type TabInfo,
} from '../src/lib/media/center';
import { chooseTarget, describeTarget, type ButtonContext } from '../src/lib/media/choose';
import type { Detection } from '../src/lib/media/detect';
import {
  MAX_SRC_LENGTH,
  MAX_TITLE_LENGTH,
  MEDIA_CHECK,
  MEDIA_DOWNLOAD,
  mediaButtonOn,
} from '../src/lib/media/messages';
import { MediaRegistry, type MediaItem, type SavedMedia } from '../src/lib/media/registry';
import { MediaWatcher } from '../src/lib/media/watch';
import { asPopupRequest } from '../src/lib/messages';
import { NativeClient } from '../src/lib/native/client';
import { FakeApp, FakeDownloads, MemoryCache, flush } from './fakes';

const HLS: Detection = {
  kind: 'hls',
  url: 'https://cdn.example.com/v/master.m3u8',
  mime: null,
  size: null,
  audio: false,
};
const DASH: Detection = {
  kind: 'dash',
  url: 'https://cdn.example.com/d/manifest.mpd',
  mime: null,
  size: null,
  audio: false,
};
const FILE: Detection = {
  kind: 'direct',
  url: 'https://files.example.com/clip.mp4',
  mime: 'video/mp4',
  size: 5_242_880,
  audio: false,
};
const SONG: Detection = {
  kind: 'direct',
  url: 'https://files.example.com/song.mp3',
  mime: 'audio/mpeg',
  size: null,
  audio: true,
};

function items(...detections: Detection[]): MediaItem[] {
  const registry = new MediaRegistry({ now: () => 1 });
  for (const d of detections) registry.add(1, d);
  return [...registry.items(1)];
}

const context = (overrides: Partial<ButtonContext> = {}): ButtonContext => ({
  elementSrc: 'blob:https://example.com/9f2',
  elementIsAudio: false,
  frameUrl: 'https://example.com/watch',
  tabUrl: 'https://example.com/watch',
  ...overrides,
});

describe('chooseTarget (what the media button sends)', () => {
  it('prefers a stream manifest, then a direct file, then the page on a well-known video site', () => {
    // The newest stream: on a page that swaps videos, the one playing now.
    expect(chooseTarget(items(FILE, HLS, DASH), context())?.url).toBe(DASH.url);
    expect(chooseTarget(items(DASH, FILE, HLS), context())?.url).toBe(HLS.url);
    expect(chooseTarget(items(FILE, DASH), context())?.url).toBe(DASH.url);
    expect(chooseTarget(items(FILE), context())?.url).toBe(FILE.url);
    expect(
      chooseTarget(
        [],
        context({
          tabUrl: 'https://www.youtube.com/watch?v=abc',
          frameUrl: 'https://www.youtube.com/watch?v=abc',
        }),
      ),
    ).toMatchObject({
      kind: 'page',
      url: 'https://www.youtube.com/watch?v=abc',
      site: 'YouTube',
    });
  });

  it("prefers media loaded by the element's own frame", () => {
    const registry = new MediaRegistry();
    registry.add(1, DASH, 5);
    registry.add(1, HLS, 0);
    registry.add(1, FILE, 0);
    registry.add(1, SONG, 5);
    const list = registry.items(1);
    expect(chooseTarget(list, context({ frameId: 5 }))?.url).toBe(DASH.url);
    expect(chooseTarget(list, context({ frameId: 0 }))?.url).toBe(HLS.url);
    // A frame with nothing of its own falls back to the whole tab.
    expect(chooseTarget(list, context({ frameId: 9 }))?.url).toBe(HLS.url);
    const files = new MediaRegistry();
    files.add(1, FILE, 0);
    files.add(1, SONG, 5);
    expect(chooseTarget(files.items(1), context({ frameId: 5 }))?.url).toBe(SONG.url);
    expect(chooseTarget(files.items(1), context({ frameId: 0 }))?.url).toBe(FILE.url);
  });

  it('passes the sender frame from the media button through', async () => {
    const { center, registry, tabs, app } = await setup();
    registry.add(1, DASH, 3);
    registry.add(1, HLS, 0);
    const sender = { ...fromTab(1, tabs, 'https://player.example.net/embed/1'), frameId: 3 };
    await center.handleContentMessage({ type: MEDIA_DOWNLOAD, src: null, audio: false, title: null }, sender);
    expect(lastAdd(app).url).toBe(DASH.url);
  });

  it('sends the embedded player page when the video sits in a frame from a well-known site', () => {
    const target = chooseTarget(
      [],
      context({
        frameUrl: 'https://www.youtube-nocookie.com/embed/abc',
        tabUrl: 'https://blog.example.com/post',
      }),
    );
    expect(target).toMatchObject({ kind: 'page', url: 'https://www.youtube-nocookie.com/embed/abc' });
  });

  it("sends the element's own file only when the browser loaded exactly that address as media", () => {
    expect(chooseTarget(items(HLS, FILE), context({ elementSrc: FILE.url }))).toMatchObject({
      kind: 'direct',
      url: FILE.url,
      size: 5_242_880,
    });
    // A page can put any address in its <video src>; one the browser never loaded is not sent.
    expect(
      chooseTarget(items(HLS), context({ elementSrc: 'https://example.com/account/export' })),
    ).toMatchObject({
      url: HLS.url,
    });
    expect(chooseTarget([], context({ elementSrc: 'https://example.com/account/export' }))).toBeNull();
    expect(chooseTarget(items(FILE), context({ elementSrc: `${FILE.url}?other=1` }))).toMatchObject({
      url: FILE.url,
    });
  });

  it('shows nothing where nothing downloadable was found', () => {
    expect(chooseTarget([], context())).toBeNull();
    expect(chooseTarget([], context({ elementSrc: 'mediasource:https://example.com/1' }))).toBeNull();
    expect(
      chooseTarget([], context({ elementSrc: null, tabUrl: 'https://www.youtube.com/', frameUrl: null })),
    ).toBeNull();
  });

  it('names each kind in plain words', () => {
    const [hls, file, song] = items(HLS, FILE, SONG).map((i) => describeTarget({ ...i, site: null }));
    expect(hls).toEqual({ title: 'Video stream (HLS)', detail: 'cdn.example.com' });
    expect(file).toEqual({ title: 'Video file', detail: 'clip.mp4' });
    expect(song).toEqual({ title: 'Audio file', detail: 'song.mp3' });
  });
});

interface Harness {
  app: FakeApp;
  hub: Hub;
  client: NativeClient;
  registry: MediaRegistry;
  center: MediaCenter;
  tabs: Map<number, TabInfo>;
  cookieCalls: Array<[string, string | null]>;
  cookieContexts: CookieContext[];
  saved: SavedMedia[];
  buttonSaves: boolean[];
}

const harnesses: Harness[] = [];

afterEach(() => {
  for (const h of harnesses.splice(0)) h.client.stop();
});

async function setup(
  options: {
    configure?: (app: FakeApp) => void;
    siteAccess?: boolean;
    saved?: unknown;
    connect?: boolean;
  } = {},
): Promise<Harness> {
  const app = new FakeApp();
  options.configure?.(app);
  const client = new NativeClient({
    transport: app,
    hello: () =>
      Promise.resolve({ client: 'extension', client_version: '0.1.0', protocol_version: 1, browser: null }),
    backoffMs: [60_000],
  });
  const registry = new MediaRegistry();
  const tabs = new Map<number, TabInfo>([
    [
      1,
      { url: 'https://example.com/watch', title: '  A   nice video ', incognito: false, cookieStoreId: null },
    ],
    [
      2,
      {
        url: 'https://example.com/private',
        title: 'Secret',
        incognito: true,
        cookieStoreId: 'firefox-private',
      },
    ],
    [3, { url: 'about:blank', title: 'New tab', incognito: false, cookieStoreId: null }],
    [
      4,
      {
        url: 'https://www.youtube.com/watch?v=abc',
        title: 'Some talk - YouTube',
        incognito: false,
        cookieStoreId: null,
      },
    ],
  ]);
  const saved: SavedMedia[] = [];
  const buttonSaves: boolean[] = [];
  const cookieCalls: Array<[string, string | null]> = [];
  const cookieContexts: CookieContext[] = [];
  const center: MediaCenter = new MediaCenter({
    registry,
    getTab: (id) => Promise.resolve(tabs.get(id) ?? null),
    send: (request) => hub.sendMedia(request),
    store: {
      load: () => Promise.resolve(options.saved ?? null),
      save: (value) => {
        saved.push(value);
        return Promise.resolve();
      },
    },
    saveDelayMs: 5,
    ignoredSites: () => hub.knownSettings()?.ignored_sites ?? [],
  });
  const hub: Hub = new Hub({
    client,
    cache: new MemoryCache(),
    downloads: new FakeDownloads(),
    cookieHeader: (url, store, context) => {
      cookieCalls.push([url, store]);
      cookieContexts.push(context);
      return Promise.resolve('session=1');
    },
    hasSiteAccess: () => Promise.resolve(options.siteAccess ?? true),
    userAgent: 'TestAgent/1.0',
    newHandoverId: () => `h${app.requests.length}`,
    media: center,
    mediaButtonSetting: {
      save: (on) => {
        buttonSaves.push(on);
        return Promise.resolve();
      },
    },
    requestTimeoutMs: 60,
    withdrawTimeoutMs: 60,
    connectWaitMs: 60,
  });
  await center.start();
  if (options.connect !== false) {
    await hub.start();
    await flush();
  }
  const h = { app, hub, client, registry, center, tabs, cookieCalls, cookieContexts, saved, buttonSaves };
  harnesses.push(h);
  return h;
}

const fromTab = (tabId: number, tabs: Map<number, TabInfo>, frameUrl?: string): ContentSender => {
  const tab = tabs.get(tabId);
  return {
    tab: {
      id: tabId,
      url: tab?.url ?? undefined,
      title: tab?.title ?? undefined,
      incognito: tab?.incognito,
      cookieStoreId: tab?.cookieStoreId ?? undefined,
    },
    url: frameUrl ?? tab?.url ?? undefined,
  };
};

function lastAdd(app: FakeApp): NewDownload {
  const call = app.calls('add_download').at(-1);
  if (call?.method !== 'add_download') throw new Error('no add_download was sent');
  return call.params;
}

describe('media button messages', () => {
  it('refuses sources that are not short web addresses, and overlong titles', async () => {
    const { center, tabs } = await setup();
    const sender = fromTab(1, tabs);
    for (const src of [
      'blob:https://example.com/1',
      'data:video/mp4;base64,AAAA',
      'javascript:alert(1)',
      `https://e.com/${'a'.repeat(MAX_SRC_LENGTH)}`,
    ]) {
      expect(center.handleContentMessage({ type: MEDIA_CHECK, src, audio: false }, sender)).toBeNull();
    }
    expect(
      center.handleContentMessage(
        { type: MEDIA_DOWNLOAD, src: null, audio: false, title: 'x'.repeat(MAX_TITLE_LENGTH + 1) },
        sender,
      ),
    ).toBeNull();
    expect(
      center.handleContentMessage({ type: MEDIA_CHECK, src: 'https://e.com/a.mp4', audio: false }, sender),
    ).not.toBeNull();
  });

  it('answers only media messages, and checks their shape', async () => {
    const { center, tabs } = await setup();
    expect(center.handleContentMessage({ type: 'something_else' }, fromTab(1, tabs))).toBeNull();
    expect(
      center.handleContentMessage({ type: MEDIA_CHECK, src: 5, audio: false }, fromTab(1, tabs)),
    ).toBeNull();
    expect(
      center.handleContentMessage({ type: MEDIA_DOWNLOAD, src: null, audio: false }, fromTab(1, tabs)),
    ).toBeNull();
  });

  it('shows the button only where something downloadable was detected, or on a well-known video site', async () => {
    const { center, registry, tabs } = await setup();
    const check = (tabId: number, src: string | null = null) =>
      center.handleContentMessage({ type: MEDIA_CHECK, src, audio: false }, fromTab(tabId, tabs));
    expect(await check(1)).toEqual({ show: false });
    registry.add(1, HLS);
    expect(await check(1)).toEqual({ show: true });
    expect(await check(4)).toEqual({ show: true });
    // An element address the browser never loaded as media does not count.
    expect(await check(3, 'https://e.com/a.mp4')).toEqual({ show: false });
    // Never in private windows, even with a playable address.
    registry.add(2, HLS);
    expect(await check(2, 'https://e.com/a.mp4')).toEqual({ show: false });
    expect(await center.handleContentMessage({ type: MEDIA_CHECK, src: null, audio: false }, {})).toEqual({
      show: false,
    });
  });

  it('hands the best hint to the app, with the media kind, the title, cookies and the page', async () => {
    const { app, center, registry, tabs, cookieCalls, cookieContexts } = await setup();
    registry.add(1, FILE);
    registry.add(1, HLS);
    const answer = await center.handleContentMessage(
      { type: MEDIA_DOWNLOAD, src: null, audio: false, title: null },
      fromTab(1, tabs, 'https://player.example.net/embed/7'),
    );
    expect(answer).toEqual({ ok: true, message: 'Sent to Wirecatch.' });
    expect(lastAdd(app)).toEqual({
      handover_id: expect.any(String) as string,
      url: HLS.url,
      // Another origin: only the frame's origin, as the browser sends it.
      referrer: 'https://player.example.net/',
      page_url: 'https://example.com/watch',
      suggested_name: null,
      mime: null,
      size_hint: null,
      cookies: 'session=1',
      user_agent: 'TestAgent/1.0',
      extra_headers: [],
      source: 'media_button',
      queue: 1,
      media: { kind: 'hls', title: 'A nice video' },
    });
    expect(cookieCalls).toEqual([[HLS.url, null]]);
    // Loaded by the player frame inside the tab's page, not a navigation.
    expect(cookieContexts).toEqual([
      {
        initiator: 'https://player.example.net/embed/7',
        topLevel: 'https://example.com/watch',
        navigation: false,
      },
    ]);
  });

  it('sends the page itself on a well-known video site, for the media add-on', async () => {
    const { app, center, tabs } = await setup();
    const answer = await center.handleContentMessage(
      { type: MEDIA_DOWNLOAD, src: null, audio: false, title: 'Some talk' },
      fromTab(4, tabs),
    );
    expect(answer).toMatchObject({ ok: true });
    expect(lastAdd(app)).toMatchObject({
      url: 'https://www.youtube.com/watch?v=abc',
      source: 'media_button',
      media: { kind: 'page', title: 'Some talk' },
    });
  });

  it('says to open Wirecatch when the app cannot be reached, and sends nothing', async () => {
    const { app, center, registry, tabs } = await setup({
      configure: (a) => (a.hostMissing = 'Specified native messaging host not found.'),
    });
    registry.add(1, HLS);
    const answer = await center.handleContentMessage(
      { type: MEDIA_DOWNLOAD, src: null, audio: false, title: null },
      fromTab(1, tabs),
    );
    expect(answer).toEqual({ ok: false, message: MEDIA_UNREACHABLE });
    expect(MEDIA_UNREACHABLE).toContain('Open Wirecatch, then try again');
    expect(app.calls('add_download')).toEqual([]);
  });

  it('withdraws a handover whose answer never came, and says nothing was added', async () => {
    const { app, center, registry, tabs } = await setup({ configure: (a) => (a.addBehavior = 'hang') });
    registry.add(1, HLS);
    const answer = await center.handleContentMessage(
      { type: MEDIA_DOWNLOAD, src: null, audio: false, title: null },
      fromTab(1, tabs),
    );
    expect(answer).toEqual({
      ok: false,
      message: 'Wirecatch did not answer in time, so nothing was added. Open Wirecatch, then try again.',
    });
    const withdraw = app.calls('withdraw_download')[0];
    expect(withdraw?.method === 'withdraw_download' && withdraw.params.handover_id).toBe(
      lastAdd(app).handover_id,
    );
  });

  it('treats a withdraw that finds the download already started as success', async () => {
    const { center, registry, tabs } = await setup({
      configure: (a) => {
        a.addBehavior = 'hang';
        a.withdrawBehavior = 'already_started';
      },
    });
    registry.add(1, HLS);
    const answer = await center.handleContentMessage(
      { type: MEDIA_DOWNLOAD, src: null, audio: false, title: null },
      fromTab(1, tabs),
    );
    expect(answer).toEqual({ ok: true, message: 'Sent to Wirecatch.' });
  });

  it('warns when even the withdraw got no answer, since the app may have it', async () => {
    const { center, registry, tabs } = await setup({
      configure: (a) => {
        a.addBehavior = 'hang';
        a.withdrawBehavior = 'hang';
      },
    });
    registry.add(1, HLS);
    const answer = await center.handleContentMessage(
      { type: MEDIA_DOWNLOAD, src: null, audio: false, title: null },
      fromTab(1, tabs),
    );
    expect(answer).toEqual({
      ok: false,
      message: 'Wirecatch did not answer in time. Open Wirecatch and check its list before trying again.',
    });
  });

  it('passes on a refusal from the app, and asks for confirmation when the app does', async () => {
    const refused = await setup({
      configure: (a) =>
        (a.addBehavior = {
          error: { code: 'invalid_request', message: 'This address cannot be downloaded' },
        }),
    });
    refused.registry.add(1, FILE);
    expect(
      await refused.center.handleContentMessage(
        { type: MEDIA_DOWNLOAD, src: null, audio: false, title: null },
        fromTab(1, refused.tabs),
      ),
    ).toEqual({ ok: false, message: 'Wirecatch did not take it. This address cannot be downloaded.' });

    const asks = await setup({ configure: (a) => (a.addBehavior = 'awaiting_confirmation') });
    asks.registry.add(1, FILE);
    expect(
      await asks.center.handleContentMessage(
        { type: MEDIA_DOWNLOAD, src: null, audio: false, title: null },
        fromTab(1, asks.tabs),
      ),
    ).toEqual({ ok: true, message: 'Sent to Wirecatch. Confirm it there to start.' });
  });

  it('refuses private windows and missing site access without sending anything', async () => {
    const { app, center, registry, tabs } = await setup();
    registry.add(2, HLS);
    expect(
      await center.handleContentMessage(
        { type: MEDIA_DOWNLOAD, src: null, audio: false, title: null },
        fromTab(2, tabs),
      ),
    ).toEqual({ ok: false, message: PRIVATE_WINDOW });

    const noAccess = await setup({ siteAccess: false });
    noAccess.registry.add(1, HLS);
    const answer = await noAccess.center.handleContentMessage(
      { type: MEDIA_DOWNLOAD, src: null, audio: false, title: null },
      fromTab(1, noAccess.tabs),
    );
    expect(answer).toMatchObject({ ok: false });
    expect((answer as { message: string }).message).toContain('Allow access');
    expect(app.calls('add_download')).toEqual([]);
    expect(noAccess.app.calls('add_download')).toEqual([]);
  });

  it('connects first when the app was not connected yet', async () => {
    const { app, hub, center, registry, tabs } = await setup({ connect: false });
    registry.add(1, HLS);
    // The hub has not started; sendMedia waits for its cache, so start it alongside.
    const answer = center.handleContentMessage(
      { type: MEDIA_DOWNLOAD, src: null, audio: false, title: null },
      fromTab(1, tabs),
    );
    await hub.start();
    expect(await answer).toEqual({ ok: true, message: 'Sent to Wirecatch.' });
    expect(app.calls('add_download')).toHaveLength(1);
  });
});

describe('popup media list', () => {
  it("lists the tab's media with labels and sizes, then the page on a well-known site", async () => {
    const { center, registry } = await setup();
    registry.add(1, HLS);
    registry.add(1, FILE);
    expect(await center.view(1)).toEqual({
      tabId: 1,
      page: 'web',
      entries: [
        {
          key: `hls:${HLS.url}`,
          kind: 'hls',
          title: 'Video stream (HLS)',
          detail: 'cdn.example.com',
          size: null,
        },
        {
          key: `direct:${FILE.url}`,
          kind: 'direct',
          title: 'Video file',
          detail: 'clip.mp4',
          size: 5_242_880,
        },
      ],
    });
    expect((await center.view(4)).entries).toEqual([
      {
        key: 'page:https://www.youtube.com/watch?v=abc',
        kind: 'page',
        title: 'This YouTube page',
        detail: 'Wirecatch reads it with its media add-on.',
        size: null,
      },
    ]);
    expect(await center.view(2)).toEqual({ tabId: 2, page: 'private', entries: [] });
    expect(await center.view(3)).toEqual({ tabId: 3, page: 'other', entries: [] });
    expect(await center.view(99)).toEqual({ tabId: 99, page: 'other', entries: [] });
  });

  it('keeps a watching popup current, and stops when it is closed', async () => {
    const { center, registry } = await setup();
    const views: MediaView[] = [];
    const stop = center.watch(1, (view) => views.push(view));
    await flush();
    registry.add(1, HLS);
    await flush();
    registry.add(5, FILE);
    registry.clearTab(1);
    await flush();
    stop();
    registry.add(1, FILE);
    await flush();
    expect(views.map((v) => v.entries.length)).toEqual([0, 1, 0]);
  });

  it('sends a picked item, and explains when it is gone or the tab is private', async () => {
    const { app, center, registry } = await setup();
    registry.add(1, FILE);
    expect(await center.download(1, `direct:${FILE.url}`)).toEqual({
      ok: true,
      message: 'Sent to Wirecatch.',
    });
    expect(lastAdd(app)).toMatchObject({
      url: FILE.url,
      referrer: 'https://example.com/',
      mime: 'video/mp4',
      size_hint: 5_242_880,
      media: { kind: 'direct', title: 'A nice video' },
    });
    expect((await center.download(1, 'direct:https://gone.example.com/x.mp4')).ok).toBe(false);
    expect(await center.download(2, 'anything')).toEqual({ ok: false, message: PRIVATE_WINDOW });
    expect(await center.download(99, 'anything')).toEqual({ ok: false, message: 'This tab is closed.' });
  });

  it('saves the lists for a restarted background script and reads them back', async () => {
    const first = await setup();
    first.registry.add(1, HLS);
    await new Promise((resolve) => setTimeout(resolve, 20));
    const saved = first.saved.at(-1);
    expect(saved?.tabs[0]?.[1].map((i) => i.url)).toEqual([HLS.url]);

    const second = await setup({ saved: JSON.parse(JSON.stringify(saved)) as unknown });
    expect(second.registry.items(1).map((i) => i.url)).toEqual([HLS.url]);
  });
});

describe('the media button setting and ignored sites', () => {
  it('publishes only the button setting, as a boolean, when it is known and when it changes', async () => {
    const { app, buttonSaves } = await setup();
    expect(buttonSaves).toEqual([true]);
    app.changeSettings({ ignored_sites: ['a.example.com'] });
    await flush();
    expect(buttonSaves).toEqual([true]);
    app.changeSettings({ show_media_button: false });
    await flush();
    expect(buttonSaves).toEqual([true, false]);
  });

  it('reads a missing or malformed value as on, and only false as off', () => {
    expect(mediaButtonOn(undefined)).toBe(true);
    expect(mediaButtonOn(true)).toBe(true);
    expect(mediaButtonOn(false)).toBe(false);
  });

  it('hides the button on ignored sites, for the page or the frame, and refuses to send from there', async () => {
    const { center, registry, tabs, app } = await setup({
      configure: (a) => (a.settings = { ...a.settings, ignored_sites: ['example.com'] }),
    });
    registry.add(1, HLS);
    registry.add(4, HLS);
    const check = (sender: ContentSender) =>
      center.handleContentMessage({ type: MEDIA_CHECK, src: null, audio: false }, sender);
    expect(await check(fromTab(1, tabs))).toEqual({ show: false });
    expect(await check(fromTab(4, tabs, 'https://player.example.com/embed/1'))).toEqual({ show: false });
    expect(await check(fromTab(4, tabs))).toEqual({ show: true });
    const answer = await center.handleContentMessage(
      { type: MEDIA_DOWNLOAD, src: null, audio: false, title: null },
      fromTab(1, tabs),
    );
    expect(answer).toMatchObject({ ok: false });
    expect(app.calls('add_download')).toEqual([]);
  });
});

describe('popup media commands', () => {
  it('accepts well-formed media commands only', () => {
    expect(asPopupRequest({ id: 1, command: { type: 'watch_media', tabId: 3 } })).not.toBeNull();
    expect(asPopupRequest({ id: 1, command: { type: 'download_media', tabId: 3, key: 'k' } })).not.toBeNull();
    expect(asPopupRequest({ id: 1, command: { type: 'watch_media', tabId: -1 } })).toBeNull();
    expect(asPopupRequest({ id: 1, command: { type: 'watch_media', tabId: '3' } })).toBeNull();
    expect(asPopupRequest({ id: 1, command: { type: 'download_media', tabId: 3 } })).toBeNull();
  });
});

describe('background restart', () => {
  const pageA = {
    kind: 'hls' as const,
    url: 'https://a.example.com/v/master.m3u8',
    mime: null,
    size: null,
    audio: false,
  };

  function savedWith(...tabs: Array<[number, Detection]>): SavedMedia {
    const before = new MediaRegistry();
    for (const [tab, d] of tabs) before.add(tab, d);
    return JSON.parse(JSON.stringify(before.save())) as SavedMedia;
  }

  it('applies the response that woke the background on top of the restored lists', async () => {
    let release: (value: unknown) => void = () => undefined;
    const registry = new MediaRegistry();
    const center = new MediaCenter({
      registry,
      getTab: () => Promise.resolve(null),
      send: () => Promise.resolve({ ok: true, message: '' }),
      store: { load: () => new Promise((resolve) => (release = resolve)), save: () => Promise.resolve() },
    });
    const restored = center.start();
    const watcher = new MediaWatcher(registry, {
      isPrivateTab: () => Promise.resolve(false),
      ready: restored,
    });
    // Tab 1 moves on to page B while the saved lists (page A) are still loading.
    const navigation = watcher.onResponse({
      url: 'https://b.example.com/',
      tabId: 1,
      type: 'main_frame',
      statusCode: 200,
      responseHeaders: [{ name: 'content-type', value: 'text/html' }],
    });
    const pageB = watcher.onResponse({
      url: 'https://b.example.com/s/master.m3u8',
      tabId: 1,
      type: 'xmlhttprequest',
      statusCode: 200,
      responseHeaders: [{ name: 'content-type', value: 'application/vnd.apple.mpegurl' }],
    });
    release(savedWith([1, pageA]));
    await Promise.all([restored, navigation, pageB]);
    expect(registry.items(1).map((i) => i.url)).toEqual(['https://b.example.com/s/master.m3u8']);
  });

  it('answers the media button only after the lists are back', async () => {
    let release: (value: unknown) => void = () => undefined;
    const registry = new MediaRegistry();
    const center = new MediaCenter({
      registry,
      getTab: () => Promise.resolve(null),
      send: () => Promise.resolve({ ok: true, message: '' }),
      store: { load: () => new Promise((resolve) => (release = resolve)), save: () => Promise.resolve() },
    });
    void center.start();
    const answer = center.handleContentMessage(
      { type: MEDIA_CHECK, src: null, audio: false },
      { tab: { id: 1, url: 'https://a.example.com/' } },
    );
    release(savedWith([1, pageA]));
    expect(await answer).toEqual({ show: true });
  });

  it('drops the saved lists of tabs that were closed meanwhile', async () => {
    const registry = new MediaRegistry();
    const center = new MediaCenter({
      registry,
      getTab: () => Promise.resolve(null),
      send: () => Promise.resolve({ ok: true, message: '' }),
      store: {
        load: () => Promise.resolve(savedWith([1, pageA], [9, pageA])),
        save: () => Promise.resolve(),
      },
      liveTabIds: () => Promise.resolve([1, 2]),
    });
    await center.start();
    expect(registry.items(1)).toHaveLength(1);
    expect(registry.items(9)).toEqual([]);
  });
});
