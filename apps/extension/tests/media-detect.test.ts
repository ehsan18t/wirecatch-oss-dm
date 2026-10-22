import { describe, expect, it } from 'vitest';
import {
  classifyResponse,
  classifyUrl,
  isNavigation,
  isWebUrl,
  MIN_DIRECT_MEDIA_BYTES,
  rejectionOf,
  type ObservedResponse,
} from '../src/lib/media/detect';
import { MAX_MEDIA_URL_LENGTH } from '../src/lib/media/messages';
import {
  HLS_STREAM_WINDOW_MS,
  MAX_ITEMS_PER_TAB,
  MAX_SAVED_CHARS,
  MediaRegistry,
} from '../src/lib/media/registry';
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
  it('says why a response can never be listed, checking in order', () => {
    expect(rejectionOf(response({}))).toBeNull();
    expect(rejectionOf(response({ tabId: -1, statusCode: 404 }))).toBe('outside_tab');
    expect(rejectionOf(response({ documentLifecycle: 'prerender' }))).toBe('prerender');
    expect(rejectionOf(response({ statusCode: 302 }))).toBe('not_success');
    expect(rejectionOf(response({ url: 'blob:https://example.com/1' }))).toBe('not_web');
    const long = `https://example.com/${'a'.repeat(MAX_MEDIA_URL_LENGTH)}.m3u8`;
    expect(rejectionOf(response({ url: long }))).toBe('url_too_long');
    const attachment = response({ headers: { 'Content-Disposition': 'attachment' } });
    expect(rejectionOf(attachment)).toBe('attachment');
  });

  it.each([
    ['application/vnd.apple.mpegurl', 'hls'],
    ['application/x-mpegURL', 'hls'],
    ['audio/mpegurl', 'hls'],
    ['application/dash+xml', 'dash'],
    ['application/dash+xml; charset=utf-8', 'dash'],
  ])(
    'spots a stream manifest by its content type %s, whatever the address',
    (contentType, kind) => {
      const hit = classifyResponse(
        response({ url: 'https://cdn.example.com/play?id=9', contentType }),
      );
      expect(hit?.kind).toBe(kind);
      expect(hit?.size).toBeNull();
    },
  );

  it.each([
    ['https://cdn.example.com/a/master.m3u8?token=1', 'text/plain', 'hls'],
    ['https://cdn.example.com/a/manifest.mpd', 'application/octet-stream', 'dash'],
    ['https://cdn.example.com/a/stream.M3U8', undefined, 'hls'],
    ['https://cdn.example.com/a/manifest.mpd', 'application/xml', 'dash'],
  ])(
    'spots a manifest by its extension when the type is generic (%s)',
    (url, contentType, kind) => {
      expect(classifyResponse(response({ url, contentType }))?.kind).toBe(kind);
    },
  );

  it('spots whole video and audio files loaded by a media element, with their size', () => {
    const video = classifyResponse(
      response({
        url: 'https://files.example.com/clip.mp4',
        type: 'media',
        contentType: 'video/mp4',
        headers: { 'Content-Length': '1234567' },
      }),
    );
    expect(video).toEqual({
      kind: 'direct',
      url: 'https://files.example.com/clip.mp4',
      mime: 'video/mp4',
      size: 1234567,
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

  // YouTube's voice search sounds, played as media on every watch page.
  it.each([
    ['no_input.mp3', 6_034],
    ['failure.mp3', 5_888],
    ['success.mp3', 6_195],
    ['open.mp3', 5_973],
  ])('ignores the interface sound %s (%i bytes), as a 200 or a 206 answer', (name, size) => {
    const url = `https://www.youtube.com/s/search/audio/${name}`;
    expect(
      classifyResponse(
        response({
          url,
          type: 'media',
          contentType: 'audio/mpeg',
          headers: { 'Content-Length': String(size) },
        }),
      ),
    ).toBeNull();
    expect(
      classifyResponse(
        response({
          url,
          type: 'media',
          statusCode: 206,
          contentType: 'audio/mpeg',
          headers: {
            'Content-Range': `bytes 0-${size - 1}/${size}`,
            'Content-Length': String(size),
          },
        }),
      ),
    ).toBeNull();
    // A generic type with the .mp3 address is the same sound.
    expect(
      classifyResponse(
        response({
          url,
          type: 'media',
          contentType: 'application/octet-stream',
          headers: { 'Content-Length': String(size) },
        }),
      ),
    ).toBeNull();
  });

  it('keeps direct files from 100 KB up, and files of unknown size', () => {
    const file = (headers: Record<string, string>) =>
      classifyResponse(
        response({
          url: 'https://files.example.com/talk.mp3',
          type: 'media',
          contentType: 'audio/mpeg',
          headers,
        }),
      );
    expect(MIN_DIRECT_MEDIA_BYTES).toBe(102_400);
    expect(file({ 'Content-Length': String(MIN_DIRECT_MEDIA_BYTES - 1) })).toBeNull();
    expect(file({ 'Content-Length': String(MIN_DIRECT_MEDIA_BYTES) })?.size).toBe(
      MIN_DIRECT_MEDIA_BYTES,
    );
    expect(file({})).toMatchObject({ kind: 'direct', size: null });
  });

  it('spots a media file opened in a tab by itself, and one with a generic type but a media extension', () => {
    expect(
      classifyResponse(
        response({ url: 'https://e.com/a.webm', type: 'main_frame', contentType: 'video/webm' }),
      )?.kind,
    ).toBe('direct');
    expect(
      classifyResponse(
        response({
          url: 'https://e.com/a.mp3',
          type: 'media',
          contentType: 'application/octet-stream',
        }),
      ),
    ).toMatchObject({ kind: 'direct', audio: true });
  });

  it('ignores media fetched by scripts, which is a slice fed to a player rather than a whole file', () => {
    expect(
      classifyResponse(
        response({
          url: 'https://e.com/v/seg.mp4',
          type: 'xmlhttprequest',
          contentType: 'video/mp4',
        }),
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
      classifyResponse(
        response({ url: 'https://e.com/', type: 'main_frame', contentType: 'text/html' }),
      ),
    ).toBeNull();
    expect(
      classifyResponse(
        response({ url: 'https://e.com/a.png', type: 'image', contentType: 'image/png' }),
      ),
    ).toBeNull();
    expect(
      classifyResponse(
        response({ url: 'https://e.com/app.js', type: 'script', contentType: 'text/javascript' }),
      ),
    ).toBeNull();
  });
});

describe('size limits', () => {
  /** A media address exactly `length` characters long. */
  const urlOf = (length: number): string => {
    const head = 'https://cdn.example.com/v/';
    const tail = '.mp4';
    return head + 'a'.repeat(length - head.length - tail.length) + tail;
  };

  it('ignores media addresses longer than 8 KB', () => {
    expect(MAX_MEDIA_URL_LENGTH).toBe(8192);
    const ok = urlOf(MAX_MEDIA_URL_LENGTH);
    const long = urlOf(MAX_MEDIA_URL_LENGTH + 1);
    expect(
      classifyResponse(response({ url: ok, type: 'media', contentType: 'video/mp4' }))?.url,
    ).toBe(ok);
    expect(
      classifyResponse(response({ url: long, type: 'media', contentType: 'video/mp4' })),
    ).toBeNull();
    const registry = new MediaRegistry();
    const detection = { kind: 'direct' as const, url: long, mime: null, size: null, audio: false };
    expect(registry.add(1, detection)).toBe(false);
    expect(registry.items(1)).toEqual([]);
  });

  it('ignores an overlong media type', () => {
    const hit = classifyResponse(
      response({
        url: 'https://cdn.example.com/v.mp4',
        type: 'media',
        contentType: `video/${'x'.repeat(300)}`,
      }),
    );
    expect(hit?.mime).toBeNull();
  });

  it('never reads back oversized addresses from saved lists', () => {
    const registry = new MediaRegistry();
    const item = (url: string) => ({
      kind: 'direct',
      url,
      key: `direct:${url}`,
      mime: null,
      size: null,
      audio: false,
      seenAt: 1,
    });
    registry.restore({
      v: 1,
      tabs: [[3, [item(urlOf(MAX_MEDIA_URL_LENGTH + 1)), item(urlOf(100))]]],
    });
    expect(registry.items(3).map((i) => i.url)).toEqual([urlOf(100)]);
  });

  it('keeps the document that requested each item across a restart, unknown for older lists', () => {
    const registry = new MediaRegistry();
    const file = (url: string) =>
      ({ kind: 'direct', url, mime: null, size: null, audio: false }) as const;
    registry.add(1, file('https://e.com/a.mp4'), 3, 'https://player.example.net');
    // Too long to keep: unknown, which the app takes as the stricter answer.
    registry.add(1, file('https://e.com/b.mp4'), 0, urlOf(MAX_MEDIA_URL_LENGTH + 1));
    const copy = new MediaRegistry();
    copy.restore(JSON.parse(JSON.stringify(registry.save())));
    expect(copy.items(1).map((i) => i.requestedBy)).toEqual(['https://player.example.net', null]);
    // A list saved before the field reads it as unknown.
    const older = new MediaRegistry();
    older.restore({
      v: 1,
      tabs: [
        [
          2,
          [
            {
              kind: 'direct',
              url: 'https://e.com/c.mp4',
              key: 'direct:https://e.com/c.mp4',
              mime: null,
              size: null,
              audio: false,
              seenAt: 1,
            },
          ],
        ],
      ],
    });
    expect(older.items(2).map((i) => i.requestedBy)).toEqual([null]);
  });

  it('keeps what it saves under the total size cap, newest tabs and items first', () => {
    let clock = 0;
    const registry = new MediaRegistry({ now: () => (clock += 1) });
    // 200 tabs of 30 near-limit addresses is about 48 MB: far over the cap and the browser's quota.
    for (let tab = 1; tab <= 200; tab += 1) {
      for (let n = 0; n < MAX_ITEMS_PER_TAB; n += 1) {
        const url = urlOf(MAX_MEDIA_URL_LENGTH - 8).replace('/v/', `/${String(tab)}/${String(n)}/`);
        expect(
          registry.add(tab, { kind: 'direct', url, mime: 'video/mp4', size: null, audio: false }),
        ).toBe(true);
      }
    }
    // Tab 5 changes last, so it is kept first.
    registry.add(5, {
      kind: 'direct',
      url: 'https://e.com/new.mp4',
      mime: null,
      size: null,
      audio: false,
    });

    const saved = registry.save();
    expect(JSON.stringify(saved).length).toBeLessThanOrEqual(MAX_SAVED_CHARS);
    expect(saved.tabs[saved.tabs.length - 1]?.[0]).toBe(5);
    const tab5 = saved.tabs[saved.tabs.length - 1]?.[1] ?? [];
    expect(tab5[tab5.length - 1]?.url).toBe('https://e.com/new.mp4');
    // Tab 200 changed just before it; the first tabs no longer fit.
    expect(saved.tabs.map(([id]) => id)).toContain(200);
    expect(saved.tabs.map(([id]) => id)).not.toContain(1);

    const copy = new MediaRegistry();
    copy.restore(JSON.parse(JSON.stringify(saved)));
    expect(copy.items(5).at(-1)?.url).toBe('https://e.com/new.mp4');
  });

  it('saves everything when it fits', () => {
    const registry = new MediaRegistry();
    registry.add(1, {
      kind: 'direct',
      url: 'https://e.com/a.mp4',
      mime: null,
      size: null,
      audio: false,
    });
    registry.add(2, {
      kind: 'direct',
      url: 'https://e.com/b.mp4',
      mime: null,
      size: null,
      audio: false,
    });
    expect(registry.save().tabs.map(([id, items]) => [id, items.length])).toEqual([
      [1, 1],
      [2, 1],
    ]);
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
      classifyResponse(
        response({ url: 'blob:https://e.com/1', type: 'media', contentType: 'video/mp4' }),
      ),
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
      isNavigation(
        response({ type: 'main_frame', headers: { 'content-disposition': 'attachment' } }),
      ),
    ).toBe(false);
  });
});

describe('MediaRegistry', () => {
  const hls = (url: string) => ({
    kind: 'hls' as const,
    url,
    mime: null,
    size: null,
    audio: false,
  });
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
        requestedBy: null,
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
    for (let i = 0; i < MAX_ITEMS_PER_TAB + 5; i += 1)
      registry.add(1, file(`https://e.com/${i}.mp4`));
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

  it('offers a list only on the page it was detected on, and keeps that page through a save and restore', () => {
    const registry = new MediaRegistry();
    // A list whose page is not known (detected before any navigation was seen) is offered anywhere, as before.
    registry.add(1, file('https://e.com/a.mp4'));
    expect(registry.itemsOn(1, 'https://e.com/anything')).toHaveLength(1);
    expect(registry.itemsOn(1, null)).toHaveLength(1);

    registry.newPage(1, 'https://e.com/watch?v=1');
    expect(registry.items(1)).toEqual([]);
    registry.add(1, file('https://e.com/b.mp4'));
    expect(registry.itemsOn(1, 'https://e.com/watch?v=1#t=5')).toHaveLength(1);
    expect(registry.itemsOn(1, 'https://e.com/watch?v=2')).toEqual([]);
    expect(registry.itemsOn(1, null)).toEqual([]);

    const saved = registry.save();
    expect(saved.tabs).toEqual([
      [1, [expect.objectContaining({ url: 'https://e.com/b.mp4' })], 'https://e.com/watch?v=1'],
    ]);
    const copy = new MediaRegistry();
    copy.restore(JSON.parse(JSON.stringify(saved)));
    expect(copy.itemsOn(1, 'https://e.com/watch?v=1')).toHaveLength(1);
    expect(copy.itemsOn(1, 'https://e.com/watch?v=2')).toEqual([]);
    // A tab that already moved to another page since the restart does not take the saved list.
    const moved = new MediaRegistry();
    moved.newPage(1, 'https://e.com/watch?v=2');
    moved.restore(JSON.parse(JSON.stringify(saved)));
    expect(moved.items(1)).toEqual([]);

    // A closed tab forgets its page too.
    registry.clearTab(1);
    registry.add(1, file('https://e.com/c.mp4'));
    expect(registry.itemsOn(1, 'https://e.com/watch?v=2')).toHaveLength(1);
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
    await watcher.onResponse(
      hlsResponse({ url: 'https://cdn.example.com/other/master.m3u8', frameId: 3 }),
    );
    expect(registry.items(4).map((i) => i.frameId)).toEqual([0, 3]);
    expect(lookups).toEqual([4]);
  });

  it('records the document that requested the media, as each browser reports it', async () => {
    const { registry, watcher } = setup();
    // Firefox names the document's address, Chromium only the origin of whoever made the request.
    await watcher.onResponse(
      hlsResponse({ documentUrl: 'https://player.example.net/embed/7', frameId: 3 }),
    );
    await watcher.onResponse(
      hlsResponse({
        url: 'https://cdn.example.com/two/master.m3u8',
        initiator: 'https://videos.example.org',
        frameId: 4,
      }),
    );
    await watcher.onResponse(
      hlsResponse({ url: 'https://cdn.example.com/three/master.m3u8', frameId: 5 }),
    );
    expect(registry.items(4).map((i) => i.requestedBy)).toEqual([
      'https://player.example.net/embed/7',
      'https://videos.example.org',
      null,
    ]);
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
      response({
        url: 'https://example.com/watch?v=1',
        type: 'main_frame',
        contentType: 'text/html',
      }),
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
    // The list is recorded for the page it was detected on.
    expect(registry.itemsOn(4, 'https://example.com/watch?v=2#t=10')).toHaveLength(1);
    expect(registry.itemsOn(4, 'https://example.com/watch?v=1')).toEqual([]);
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
