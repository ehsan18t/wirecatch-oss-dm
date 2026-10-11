import { describe, expect, it } from 'vitest';
import {
  classifyResponse,
  classifyUrl,
  isNavigation,
  isWebUrl,
  type ObservedResponse,
} from '../src/lib/media/detect';
import { pageSiteFor } from '../src/lib/media/page-sites';
import { HLS_STREAM_WINDOW_MS, MAX_ITEMS_PER_TAB, MediaRegistry } from '../src/lib/media/registry';
import { MediaWatcher } from '../src/lib/media/watch';
import { flush } from './fakes';

function response(
  overrides: Partial<ObservedResponse> & { contentType?: string; headers?: Record<string, string> },
): ObservedResponse {
  const { contentType, headers = {}, ...rest } = overrides;
  const all = contentType === undefined ? headers : { 'Content-Type': contentType, ...headers };
  return {
    url: 'https://cdn.example.com/video/master.m3u8',
    tabId: 4,
    type: 'xmlhttprequest',
    statusCode: 200,
    responseHeaders: Object.entries(all).map(([name, value]) => ({ name, value })),
    ...rest,
  };
}

describe('classifyResponse', () => {
  it.each([
    ['application/vnd.apple.mpegurl', 'hls'],
    ['application/x-mpegURL', 'hls'],
    ['audio/mpegurl', 'hls'],
    ['application/dash+xml', 'dash'],
    ['application/dash+xml; charset=utf-8', 'dash'],
  ])('spots a stream manifest by its content type %s, whatever the address', (contentType, kind) => {
    const hit = classifyResponse(response({ url: 'https://cdn.example.com/play?id=9', contentType }));
    expect(hit?.kind).toBe(kind);
    expect(hit?.size).toBeNull();
  });

  it.each([
    ['https://cdn.example.com/a/master.m3u8?token=1', 'text/plain', 'hls'],
    ['https://cdn.example.com/a/manifest.mpd', 'application/octet-stream', 'dash'],
    ['https://cdn.example.com/a/stream.M3U8', undefined, 'hls'],
    ['https://cdn.example.com/a/manifest.mpd', 'application/xml', 'dash'],
  ])('spots a manifest by its extension when the type is generic (%s)', (url, contentType, kind) => {
    expect(classifyResponse(response({ url, contentType }))?.kind).toBe(kind);
  });

  it('spots whole video and audio files loaded by a media element, with their size', () => {
    const video = classifyResponse(
      response({
        url: 'https://files.example.com/clip.mp4',
        type: 'media',
        contentType: 'video/mp4',
        headers: { 'Content-Length': '1234' },
      }),
    );
    expect(video).toEqual({
      kind: 'direct',
      url: 'https://files.example.com/clip.mp4',
      mime: 'video/mp4',
      size: 1234,
      audio: false,
    });

    const audio = classifyResponse(
      response({
        url: 'https://files.example.com/song',
        type: 'media',
        statusCode: 206,
        contentType: 'audio/mpeg',
        headers: { 'Content-Range': 'bytes 0-1023/987654', 'Content-Length': '1024' },
      }),
    );
    expect(audio).toMatchObject({ kind: 'direct', audio: true, size: 987654 });
  });

  it('spots a media file opened in a tab by itself, and one with a generic type but a media extension', () => {
    expect(
      classifyResponse(
        response({ url: 'https://e.com/a.webm', type: 'main_frame', contentType: 'video/webm' }),
      )?.kind,
    ).toBe('direct');
    expect(
      classifyResponse(
        response({ url: 'https://e.com/a.mp3', type: 'media', contentType: 'application/octet-stream' }),
      ),
    ).toMatchObject({ kind: 'direct', audio: true });
  });

  it('ignores media fetched by scripts, which is a slice fed to a player rather than a whole file', () => {
    expect(
      classifyResponse(
        response({ url: 'https://e.com/v/seg.mp4', type: 'xmlhttprequest', contentType: 'video/mp4' }),
      ),
    ).toBeNull();
  });

  it.each([
    ['https://e.com/v/seg-1.ts', 'video/mp2t'],
    ['https://e.com/v/chunk-2.m4s', 'video/iso.segment'],
    ['https://e.com/v/chunk-3.m4s', 'video/mp4'],
  ])('ignores stream segments (%s)', (url, contentType) => {
    expect(classifyResponse(response({ url, type: 'media', contentType }))).toBeNull();
  });

  it('ignores error answers, redirects, downloads, prerendered pages and requests outside a tab', () => {
    const hls = { contentType: 'application/vnd.apple.mpegurl' };
    expect(classifyResponse(response({ ...hls, statusCode: 404 }))).toBeNull();
    expect(classifyResponse(response({ ...hls, statusCode: 302 }))).toBeNull();
    expect(classifyResponse(response({ ...hls, tabId: -1 }))).toBeNull();
    expect(classifyResponse(response({ ...hls, documentLifecycle: 'prerender' }))).toBeNull();
    expect(
      classifyResponse(
        response({
          url: 'https://e.com/a.mp4',
          type: 'main_frame',
          contentType: 'video/mp4',
          headers: { 'Content-Disposition': 'attachment; filename=a.mp4' },
        }),
      ),
    ).toBeNull();
  });

  it('ignores ordinary pages, images and scripts', () => {
    expect(
      classifyResponse(response({ url: 'https://e.com/', type: 'main_frame', contentType: 'text/html' })),
    ).toBeNull();
    expect(
      classifyResponse(response({ url: 'https://e.com/a.png', type: 'image', contentType: 'image/png' })),
    ).toBeNull();
    expect(
      classifyResponse(
        response({ url: 'https://e.com/app.js', type: 'script', contentType: 'text/javascript' }),
      ),
    ).toBeNull();
  });
});

describe('manifest addresses with the wrong kind of answer', () => {
  it.each([
    ['https://cdn.example.com/a/master.m3u8', 'text/html'],
    ['https://cdn.example.com/a/master.m3u8', 'application/json'],
    ['https://cdn.example.com/a/manifest.mpd', 'text/html'],
    ['https://cdn.example.com/a/manifest.mpd', 'application/json'],
  ])('does not take %s answered as %s for a manifest', (url, contentType) => {
    expect(classifyResponse(response({ url, contentType }))).toBeNull();
  });
});

describe('addresses', () => {
  it('ignores blob, data and mediasource sources, which cannot be downloaded from outside the page', () => {
    for (const src of [
      'blob:https://e.com/1b2c',
      'mediasource:https://e.com/1',
      'data:video/mp4;base64,AAAA',
      'ftp://e.com/a.mp4',
    ]) {
      expect(isWebUrl(src)).toBe(false);
      expect(classifyUrl(src)).toBeNull();
    }
    expect(
      classifyResponse(response({ url: 'blob:https://e.com/1', type: 'media', contentType: 'video/mp4' })),
    ).toBeNull();
  });

  it('reads the kind from the extension', () => {
    expect(classifyUrl('https://e.com/a/b.m3u8?x=1')).toEqual({ kind: 'hls', audio: false });
    expect(classifyUrl('https://e.com/a/b.mpd')).toEqual({ kind: 'dash', audio: false });
    expect(classifyUrl('https://e.com/a/b.mkv')).toEqual({ kind: 'direct', audio: false });
    expect(classifyUrl('https://e.com/a/b.flac')).toEqual({ kind: 'direct', audio: true });
    expect(classifyUrl('https://e.com/a/page.html')).toBeNull();
    expect(classifyUrl('https://e.com/watch')).toBeNull();
  });

  it('treats a successful page load in a tab as a navigation, but not a download or a frame', () => {
    expect(isNavigation(response({ type: 'main_frame', contentType: 'text/html' }))).toBe(true);
    expect(isNavigation(response({ type: 'sub_frame', contentType: 'text/html' }))).toBe(false);
    expect(isNavigation(response({ type: 'main_frame', statusCode: 500 }))).toBe(false);
    expect(
      isNavigation(response({ type: 'main_frame', headers: { 'content-disposition': 'attachment' } })),
    ).toBe(false);
  });
});

describe('MediaRegistry', () => {
  const hls = (url: string) => ({ kind: 'hls' as const, url, mime: null, size: null, audio: false });
  const file = (url: string, size: number | null = null) => ({
    kind: 'direct' as const,
    url,
    mime: 'video/mp4',
    size,
    audio: false,
  });

  it('lists each file once per tab, keeping the newest address and any size learned later', () => {
    const registry = new MediaRegistry({ now: () => 5 });
    expect(registry.add(1, file('https://e.com/a.mp4?t=1'))).toBe(true);
    expect(registry.add(1, file('https://e.com/a.mp4?t=1'))).toBe(false);
    expect(registry.add(1, file('https://e.com/a.mp4?t=2', 900))).toBe(true);
    expect(registry.items(1)).toEqual([
      {
        kind: 'direct',
        url: 'https://e.com/a.mp4?t=2',
        mime: 'video/mp4',
        size: 900,
        audio: false,
        key: 'direct:https://e.com/a.mp4',
        seenAt: 5,
        frameId: 0,
        lastActive: 5,
      },
    ]);
    // Another tab has its own list.
    expect(registry.add(2, file('https://e.com/a.mp4?t=1'))).toBe(true);
    expect(registry.items(2)).toHaveLength(1);
  });

  it('lists only the first playlist of an HLS stream, wherever its variants live', () => {
    let clock = 0;
    const registry = new MediaRegistry({ now: () => clock });
    registry.add(1, hls('https://cdn.e.com/v/abc/master.m3u8'), 0);
    clock = 800;
    expect(registry.add(1, hls('https://cdn.e.com/v/abc/720p/index.m3u8'), 0)).toBe(false);
    // Variants in another folder or on another server are still the same stream.
    clock = 1_500;
    expect(registry.add(1, hls('https://edge2.e.net/x/y/480.m3u8'), 0)).toBe(false);
    // A live stream keeps requesting its variants, which keeps the stream open.
    for (let i = 0; i < 5; i += 1) {
      clock += HLS_STREAM_WINDOW_MS - 1;
      expect(registry.add(1, hls('https://cdn.e.com/v/abc/720p/index.m3u8'), 0)).toBe(false);
    }
    expect(registry.items(1).map((i) => i.url)).toEqual(['https://cdn.e.com/v/abc/master.m3u8']);
  });

  it('keeps streams from other frames, or that start after a quiet stretch, as separate entries', () => {
    let clock = 0;
    const registry = new MediaRegistry({ now: () => clock });
    // A master at the root of a server must not swallow another player's stream there.
    registry.add(1, hls('https://cdn.e.com/master.m3u8'), 0);
    expect(registry.add(1, hls('https://cdn.e.com/v/def/master.m3u8'), 7)).toBe(true);
    clock = HLS_STREAM_WINDOW_MS + 1;
    expect(registry.add(1, hls('https://cdn.e.com/v/ghi/master.m3u8'), 0)).toBe(true);
    expect(registry.items(1).map((i) => [i.url, i.frameId])).toEqual([
      ['https://cdn.e.com/master.m3u8', 0],
      ['https://cdn.e.com/v/def/master.m3u8', 7],
      ['https://cdn.e.com/v/ghi/master.m3u8', 0],
    ]);
  });

  it('keeps at most a fixed number of items per tab, dropping the oldest to make room for new media', () => {
    const registry = new MediaRegistry();
    for (let i = 0; i < MAX_ITEMS_PER_TAB + 5; i += 1) registry.add(1, file(`https://e.com/${i}.mp4`));
    expect(registry.items(1)).toHaveLength(MAX_ITEMS_PER_TAB);
    expect(registry.items(1)[0]?.url).toBe('https://e.com/5.mp4');
    expect(registry.items(1).at(-1)?.url).toBe(`https://e.com/${MAX_ITEMS_PER_TAB + 4}.mp4`);
  });

  it('tells listeners which tab changed, and survives a save and restore', () => {
    const registry = new MediaRegistry();
    const changed: number[] = [];
    registry.onChange((tab) => changed.push(tab));
    registry.add(3, file('https://e.com/a.mp4'));
    registry.clearTab(3);
    registry.clearTab(3);
    expect(changed).toEqual([3, 3]);

    registry.add(7, file('https://e.com/b.mp4'));
    const copy = new MediaRegistry();
    copy.add(7, file('https://e.com/c.mp4'));
    copy.restore(JSON.parse(JSON.stringify(registry.save())));
    expect(copy.items(7).map((i) => i.url)).toEqual(['https://e.com/b.mp4', 'https://e.com/c.mp4']);
    copy.restore({ v: 2, tabs: [] });
    copy.restore({ v: 1, tabs: [[8, [{ kind: 'nonsense' }]]] });
    expect(copy.items(8)).toEqual([]);
  });
});

describe('MediaWatcher', () => {
  const hlsResponse = (overrides: Partial<ObservedResponse> = {}) =>
    response({ contentType: 'application/vnd.apple.mpegurl', ...overrides });

  function setup(isPrivateTab: (tabId: number) => Promise<boolean> = () => Promise.resolve(false)) {
    const registry = new MediaRegistry();
    const lookups: number[] = [];
    const watcher = new MediaWatcher(registry, {
      isPrivateTab: (tabId) => {
        lookups.push(tabId);
        return isPrivateTab(tabId);
      },
    });
    return { registry, watcher, lookups };
  }

  it('records media per tab and asks whether a tab is private only once', async () => {
    const { registry, watcher, lookups } = setup();
    await watcher.onResponse(hlsResponse());
    await watcher.onResponse(hlsResponse({ url: 'https://cdn.example.com/other/master.m3u8', frameId: 3 }));
    expect(registry.items(4).map((i) => i.frameId)).toEqual([0, 3]);
    expect(lookups).toEqual([4]);
  });

  it('clears a tab when it loads a new page, and keeps a media file opened as the page itself', async () => {
    const { registry, watcher } = setup();
    await watcher.onResponse(hlsResponse());
    await watcher.onResponse(
      response({ url: 'https://example.com/next', type: 'main_frame', contentType: 'text/html' }),
    );
    expect(registry.items(4)).toEqual([]);
    await watcher.onResponse(
      response({ url: 'https://example.com/a.mp4', type: 'main_frame', contentType: 'video/mp4' }),
    );
    expect(registry.items(4).map((i) => i.url)).toEqual(['https://example.com/a.mp4']);
  });

  it('clears a tab whose address changes without a new page (history API, back-forward cache)', async () => {
    const { registry, watcher } = setup();
    await watcher.onResponse(
      response({ url: 'https://example.com/watch?v=1', type: 'main_frame', contentType: 'text/html' }),
    );
    await watcher.onResponse(hlsResponse());
    // The address the page loaded with, reported again: nothing changes.
    await watcher.onUrlChange(4, 'https://example.com/watch?v=1');
    // Only the fragment changes: still the same page.
    await watcher.onUrlChange(4, 'https://example.com/watch?v=1#comments');
    expect(registry.items(4)).toHaveLength(1);
    // The page swaps to another video with the history API.
    await watcher.onUrlChange(4, 'https://example.com/watch?v=2');
    expect(registry.items(4)).toEqual([]);
    await watcher.onResponse(hlsResponse({ url: 'https://cdn.example.com/v2/master.m3u8' }));
    await watcher.onUrlChange(4, 'https://example.com/watch?v=2#t=10');
    expect(registry.items(4).map((i) => i.url)).toEqual(['https://cdn.example.com/v2/master.m3u8']);
  });

  it('forgets a closed tab', async () => {
    const { registry, watcher } = setup();
    await watcher.onResponse(hlsResponse());
    watcher.onTabRemoved(4);
    expect(registry.items(4)).toEqual([]);
  });

  it('ignores private windows: Firefox marks the request, Chromium is asked through the tab', async () => {
    const firefox = setup();
    await firefox.watcher.onResponse(hlsResponse({ incognito: true }));
    expect(firefox.registry.items(4)).toEqual([]);
    expect(firefox.lookups).toEqual([]);
    await firefox.watcher.onResponse(hlsResponse({ incognito: false }));
    expect(firefox.registry.items(4)).toHaveLength(1);

    const chromium = setup(() => Promise.resolve(true));
    await chromium.watcher.onResponse(hlsResponse());
    expect(chromium.registry.items(4)).toEqual([]);

    const unknownTab = setup(() => Promise.reject(new Error('No tab with id: 4')));
    await unknownTab.watcher.onResponse(hlsResponse());
    expect(unknownTab.registry.items(4)).toEqual([]);
  });

  it('ignores requests that do not belong to a tab without asking anything', async () => {
    const { registry, watcher, lookups } = setup();
    await watcher.onResponse(hlsResponse({ tabId: -1 }));
    expect(lookups).toEqual([]);
    expect(registry.save().tabs).toEqual([]);
  });

  it('drops media from the old page that was still being checked when the tab moved on', async () => {
    let answer: (value: boolean) => void = () => undefined;
    const { registry, watcher } = setup(() => new Promise((resolve) => (answer = resolve)));
    const pending = watcher.onResponse(hlsResponse());
    await watcher.onResponse(
      response({ url: 'https://example.com/next', type: 'main_frame', contentType: 'text/html' }),
    );
    answer(false);
    await pending;
    await flush();
    expect(registry.items(4)).toEqual([]);
  });
});

describe('pageSiteFor', () => {
  it.each([
    ['https://www.youtube.com/watch?v=abc', 'YouTube'],
    ['https://m.youtube.com/shorts/abc', 'YouTube'],
    ['https://youtu.be/abc', 'YouTube'],
    ['https://vimeo.com/123456', 'Vimeo'],
    ['https://www.dailymotion.com/video/x8abc', 'Dailymotion'],
    ['https://www.twitch.tv/videos/123', 'Twitch'],
    ['https://clips.twitch.tv/SomeClip', 'Twitch'],
    ['https://x.com/someone/status/123', 'X'],
    ['https://twitter.com/someone/status/123', 'X'],
    ['https://www.facebook.com/watch/?v=1', 'Facebook'],
    ['https://www.instagram.com/reel/abc/', 'Instagram'],
    ['https://www.tiktok.com/@someone/video/123', 'TikTok'],
    ['https://www.reddit.com/r/videos/comments/abc/title/', 'Reddit'],
    ['https://soundcloud.com/artist/track', 'SoundCloud'],
  ])('offers %s as a page for the media add-on', (url, name) => {
    expect(pageSiteFor(url)).toBe(name);
  });

  it.each([
    'https://www.youtube.com/',
    'https://www.youtube.com/results?search_query=x',
    'https://vimeo.com/',
    'https://x.com/home',
    'https://notyoutube.com/watch?v=abc',
    'https://example.com/watch?v=abc',
    'file:///C:/video.mp4',
    'not a url',
    null,
  ])('does not offer %s', (url) => {
    expect(pageSiteFor(url)).toBeNull();
  });
});
