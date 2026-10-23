/**
 * How the background registers the media side's browser listeners: request filters,
 * browser differences and sender checks, with fake event objects.
 */
import { describe, expect, it } from 'vitest';
import { MatchPattern } from 'wxt/utils/match-patterns';
import { MediaCenter, type ContentSender, type MediaSendResult } from '../src/lib/media/center';
import type { ObservedRequest, ObservedResponse } from '../src/lib/media/detect';
import {
  FILE_REQUESTS,
  MANIFEST_REQUESTS,
  MEDIA_ELEMENT_REQUESTS,
  registerMediaListeners,
  type MediaEventApi,
  type RequestFilter,
} from '../src/lib/media/listen';
import { MEDIA_CHECK, MEDIA_COMMAND } from '../src/lib/media/messages';
import { MediaRegistry } from '../src/lib/media/registry';
import { MAX_UNANSWERED_REQUESTS, MediaWatcher } from '../src/lib/media/watch';
import { POPUP_PORT_NAME, acceptsPopupPort } from '../src/lib/messages';
import { flush } from './fakes';

type MessageListener = (
  message: unknown,
  sender: ContentSender & { id?: string },
  sendResponse: (response: unknown) => void,
) => boolean | undefined;

function fakeApi(firefox = false) {
  const requests: {
    listener: (d: ObservedRequest) => void;
    filter: RequestFilter;
    extra: string[];
  }[] = [];
  const responses: {
    listener: (d: ObservedResponse) => void;
    filter: RequestFilter;
    extra: string[];
  }[] = [];
  const removed: ((tabId: number) => void)[] = [];
  const updated: {
    listener: (tabId: number, change: { url?: string }) => void;
    filter: unknown;
  }[] = [];
  const messages: MessageListener[] = [];
  const commands: ((command: string, tab?: { id?: number }) => void)[] = [];
  const api: MediaEventApi = {
    onSendHeaders: {
      addListener: (listener, filter, extra) => void requests.push({ listener, filter, extra }),
    },
    onResponseStarted: {
      addListener: (listener, filter, extra) => void responses.push({ listener, filter, extra }),
    },
    onTabRemoved: { addListener: (listener) => void removed.push(listener) },
    onTabUpdated: {
      addListener: (listener, ...rest: unknown[]) =>
        void updated.push({ listener, filter: rest.length > 0 ? rest[0] : 'none' }),
    },
    onMessage: { addListener: (listener) => void messages.push(listener) },
    onCommand: { addListener: (listener) => void commands.push(listener) },
    extensionId: 'wirecatch-id',
    firefox,
  };
  return { api, requests, responses, removed, updated, messages, commands };
}

function setup(firefox = false) {
  const fake = fakeApi(firefox);
  const registry = new MediaRegistry();
  const watcher = new MediaWatcher(registry, { isPrivateTab: () => Promise.resolve(false) });
  const sent: string[] = [];
  const announced: [number, MediaSendResult][] = [];
  const media = new MediaCenter({
    registry,
    getTab: (id) =>
      Promise.resolve(
        id === 99
          ? null
          : {
              url: 'https://example.com/watch',
              title: 'A video',
              incognito: false,
              cookieStoreId: null,
            },
      ),
    send: (request) => {
      sent.push(request.target.url);
      return Promise.resolve({ ok: true, message: 'Sent to Wirecatch.' });
    },
  });
  registerMediaListeners(fake.api, {
    watcher,
    media,
    announce: (tabId, result) => announced.push([tabId, result]),
  });
  return { ...fake, registry, sent, announced };
}

function matches(filter: RequestFilter, url: string): boolean {
  return filter.urls.some((pattern) => new MatchPattern(pattern).includes(url));
}

describe('the keyboard shortcut', () => {
  it("sends the tab's best media and shows the answer on that tab", async () => {
    const { commands, registry, sent, announced } = setup();
    registry.add(5, {
      kind: 'hls',
      url: 'https://cdn.example.com/a/master.m3u8',
      mime: null,
      size: null,
      audio: false,
    });
    registry.add(5, {
      kind: 'dash',
      url: 'https://cdn.example.com/b/manifest.mpd',
      mime: null,
      size: null,
      audio: false,
    });
    commands[0]?.(MEDIA_COMMAND, { id: 5 });
    await flush();
    expect(sent).toEqual(['https://cdn.example.com/b/manifest.mpd']);
    expect(announced).toEqual([[5, { ok: true, message: 'Sent to Wirecatch.' }]]);
  });

  it('says so when nothing was found or the tab is gone, and ignores other commands', async () => {
    const { commands, sent, announced } = setup();
    commands[0]?.(MEDIA_COMMAND, { id: 6 });
    commands[0]?.(MEDIA_COMMAND, { id: 99 });
    commands[0]?.('something-else', { id: 6 });
    commands[0]?.(MEDIA_COMMAND, {});
    await flush();
    expect(sent).toEqual([]);
    expect(announced.map(([tab, r]) => [tab, r.ok])).toEqual([
      [6, false],
      [99, false],
    ]);
    expect(announced[0]?.[1].message).toContain('has not found anything');
  });
});

describe('registerMediaListeners', () => {
  it('observes media and page loads everywhere, but script requests only for manifest addresses', () => {
    const { responses } = setup();
    expect(responses.map((r) => r.filter)).toEqual([FILE_REQUESTS, MANIFEST_REQUESTS]);
    expect(responses.every((r) => r.extra.join() === 'responseHeaders')).toBe(true);
    expect(FILE_REQUESTS.types).not.toContain('xmlhttprequest');
    expect(FILE_REQUESTS.urls).toEqual(['http://*/*', 'https://*/*']);
    expect(MANIFEST_REQUESTS.types).toEqual(['xmlhttprequest', 'other']);
    expect(matches(MANIFEST_REQUESTS, 'https://cdn.example.com/v/master.m3u8')).toBe(true);
    expect(matches(MANIFEST_REQUESTS, 'https://cdn.example.com/d/manifest.mpd')).toBe(true);
    expect(matches(MANIFEST_REQUESTS, 'https://cdn.example.com/v/index.m3u8?token=abc')).toBe(true);
    expect(matches(MANIFEST_REQUESTS, 'https://api.example.com/v1/feed.json')).toBe(false);
    expect(matches(MANIFEST_REQUESTS, 'https://cdn.example.com/v/seg-1.ts')).toBe(false);
  });

  it('never asks to block or change requests', () => {
    const { requests, responses } = setup();
    for (const r of [...requests, ...responses]) expect(r.extra).not.toContain('blocking');
  });

  it('reads the headers of media and manifest requests, never page loads, with the Cookie header in Chromium too', () => {
    expect(MEDIA_ELEMENT_REQUESTS.types).toEqual(['media', 'object']);
    for (const firefox of [false, true]) {
      const { requests } = setup(firefox);
      expect(requests.map((r) => r.filter)).toEqual([MEDIA_ELEMENT_REQUESTS, MANIFEST_REQUESTS]);
      const extra = firefox ? ['requestHeaders'] : ['requestHeaders', 'extraHeaders'];
      for (const r of requests) expect(r.extra).toEqual(extra);
    }
  });

  it('feeds every observed response to the watcher', async () => {
    const { responses, registry } = setup();
    responses[1]?.listener({
      requestId: '1',
      url: 'https://cdn.example.com/v/master.m3u8',
      tabId: 3,
      type: 'xmlhttprequest',
      statusCode: 200,
      responseHeaders: [{ name: 'content-type', value: 'application/vnd.apple.mpegurl' }],
    });
    await flush();
    expect(registry.items(3)).toHaveLength(1);
  });
});

describe('the Cookie header sent with media', () => {
  const manifest = (url: string, requestId: string): ObservedResponse => ({
    requestId,
    url,
    tabId: 3,
    type: 'xmlhttprequest',
    statusCode: 200,
    responseHeaders: [{ name: 'content-type', value: 'application/dash+xml' }],
  });
  const request = (requestId: string, cookie?: string, tabId = 3): ObservedRequest => ({
    requestId,
    tabId,
    requestHeaders: [
      { name: 'Accept', value: '*/*' },
      ...(cookie === undefined ? [] : [{ name: 'Cookie', value: cookie }]),
    ],
  });
  /** Sends requests with ids `from` up to `to`, each with a cookie of its own. */
  const fill = (send: (r: ObservedRequest) => void, from: number, to: number): void => {
    for (let id = from; id <= to; id += 1) send(request(String(id), `n=${String(id)}`));
  };

  it('is kept with the media its request loaded, and none sent is kept as none', async () => {
    const { requests, responses, registry } = setup();
    requests[1]?.listener(request('7', 'sid=2; prefs=dark'));
    requests[1]?.listener(request('8'));
    responses[1]?.listener(manifest('https://cdn.example.com/a/manifest.mpd', '7'));
    responses[1]?.listener(manifest('https://cdn.example.com/b/manifest.mpd', '8'));
    // A response whose request was not seen says nothing about its cookies.
    responses[1]?.listener(manifest('https://cdn.example.com/c/manifest.mpd', 'never-seen'));
    await flush();
    expect(registry.items(3).map((i) => i.sentCookies)).toEqual([
      'sid=2; prefs=dark',
      null,
      undefined,
    ]);
  });

  it('is never held for requests outside a tab', async () => {
    const { requests, responses, registry } = setup();
    requests[1]?.listener(request('9', 'worker=1', -1));
    responses[1]?.listener(manifest('https://cdn.example.com/a/manifest.mpd', '9'));
    await flush();
    expect(registry.items(3).map((i) => i.sentCookies)).toEqual([undefined]);
  });

  it(`is forgotten for requests no answer came for, beyond the newest ${String(MAX_UNANSWERED_REQUESTS)}`, async () => {
    const { requests, responses, registry } = setup();
    const send = (r: ObservedRequest) => requests[1]?.listener(r);
    fill(send, 0, MAX_UNANSWERED_REQUESTS);
    const last = String(MAX_UNANSWERED_REQUESTS);
    responses[1]?.listener(manifest('https://cdn.example.com/a/manifest.mpd', '0'));
    responses[1]?.listener(manifest('https://cdn.example.com/b/manifest.mpd', last));
    await flush();
    expect(registry.items(3).map((i) => i.sentCookies)).toEqual([undefined, `n=${last}`]);
  });

  it('is the last hop’s after a redirect, which counts as the newest request', async () => {
    const { requests, responses, registry } = setup();
    const send = (r: ObservedRequest) => requests[1]?.listener(r);
    send(request('r', 'hop=1'));
    fill(send, 1, MAX_UNANSWERED_REQUESTS - 1);
    // The redirect sends the same request again, to the new address.
    send(request('r', 'hop=2'));
    fill(send, MAX_UNANSWERED_REQUESTS, MAX_UNANSWERED_REQUESTS);
    responses[1]?.listener(manifest('https://cdn.example.com/a/manifest.mpd', 'r'));
    await flush();
    expect(registry.items(3).map((i) => i.sentCookies)).toEqual(['hop=2']);
  });
});

describe('tab and message listeners', () => {
  it('limits address change events to the address in Firefox, and passes no filter in Chromium', () => {
    expect(setup(true).updated.map((u) => u.filter)).toEqual([{ properties: ['url'] }]);
    expect(setup(false).updated.map((u) => u.filter)).toEqual(['none']);
  });

  it('clears a tab when its address changes or it closes', async () => {
    const { updated, removed, registry } = setup();
    const hls = {
      kind: 'hls' as const,
      url: 'https://cdn.example.com/a/master.m3u8',
      mime: null,
      size: null,
      audio: false,
    };
    registry.add(5, hls);
    updated[0]?.listener(5, { url: 'https://example.com/next' });
    await flush();
    expect(registry.items(5)).toEqual([]);
    registry.add(5, hls);
    updated[0]?.listener(5, { title: 'Only the title' } as { url?: string });
    await flush();
    expect(registry.items(5)).toHaveLength(1);
    removed[0]?.(5);
    expect(registry.items(5)).toEqual([]);
  });

  it("answers media questions only from this extension's content scripts", async () => {
    const { messages } = setup();
    const listener = messages[0];
    if (listener === undefined) throw new Error('no message listener');
    const answers: unknown[] = [];
    const ask = (
      sender: ContentSender & { id?: string },
      message: unknown = { type: MEDIA_CHECK, src: null, audio: false },
    ) => listener(message, sender, (answer) => answers.push(answer));
    const tab = { id: 4, url: 'https://example.com/watch' };
    expect(ask({ id: 'another-extension', tab })).toBeUndefined();
    expect(ask({ id: 'wirecatch-id' })).toBeUndefined();
    expect(ask({ id: 'wirecatch-id', tab }, { type: 'not-for-media' })).toBeUndefined();
    expect(answers).toEqual([]);
    expect(ask({ id: 'wirecatch-id', tab })).toBe(true);
    await flush();
    expect(answers).toEqual([{ show: false }]);
  });
});

describe('acceptsPopupPort', () => {
  const base = 'chrome-extension://wirecatch-id/';
  it('accepts the popup page of this extension', () => {
    expect(
      acceptsPopupPort(
        { name: POPUP_PORT_NAME, sender: { id: 'wirecatch-id', url: `${base}popup.html` } },
        'wirecatch-id',
        base,
      ),
    ).toBe(true);
  });

  it('refuses content scripts, other extensions and other port names', () => {
    expect(
      acceptsPopupPort(
        { name: POPUP_PORT_NAME, sender: { id: 'wirecatch-id', url: 'https://evil.example.com/' } },
        'wirecatch-id',
        base,
      ),
    ).toBe(false);
    expect(
      acceptsPopupPort(
        {
          name: POPUP_PORT_NAME,
          sender: { id: 'other', url: 'chrome-extension://other/popup.html' },
        },
        'wirecatch-id',
        base,
      ),
    ).toBe(false);
    expect(
      acceptsPopupPort(
        { name: POPUP_PORT_NAME, sender: { id: 'wirecatch-id' } },
        'wirecatch-id',
        base,
      ),
    ).toBe(false);
    expect(acceptsPopupPort({ name: POPUP_PORT_NAME }, 'wirecatch-id', base)).toBe(false);
    expect(
      acceptsPopupPort(
        { name: 'other', sender: { id: 'wirecatch-id', url: `${base}popup.html` } },
        'wirecatch-id',
        base,
      ),
    ).toBe(false);
  });
});
