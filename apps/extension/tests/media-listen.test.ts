/**
 * How the background registers the media side's browser listeners: request filters,
 * browser differences and sender checks, with fake event objects.
 */
import { describe, expect, it } from 'vitest';
import { MatchPattern } from 'wxt/utils/match-patterns';
import { MediaCenter, type ContentSender } from '../src/lib/media/center';
import type { ObservedResponse } from '../src/lib/media/detect';
import {
  FILE_REQUESTS,
  MANIFEST_REQUESTS,
  registerMediaListeners,
  type MediaEventApi,
  type RequestFilter,
} from '../src/lib/media/listen';
import { MediaRegistry } from '../src/lib/media/registry';
import { MediaWatcher } from '../src/lib/media/watch';
import { flush } from './fakes';

type MessageListener = (
  message: unknown,
  sender: ContentSender & { id?: string },
  sendResponse: (response: unknown) => void,
) => boolean | undefined;

function fakeApi(firefox = false) {
  const responses: Array<{
    listener: (d: ObservedResponse) => void;
    filter: RequestFilter;
    extra: string[];
  }> = [];
  const removed: Array<(tabId: number) => void> = [];
  const updated: Array<{ listener: (tabId: number, change: { url?: string }) => void; filter: unknown }> = [];
  const messages: MessageListener[] = [];
  const api: MediaEventApi = {
    onResponseStarted: {
      addListener: (listener, filter, extra) => void responses.push({ listener, filter, extra }),
    },
    onTabRemoved: { addListener: (listener) => void removed.push(listener) },
    onTabUpdated: {
      addListener: (listener, ...rest: unknown[]) =>
        void updated.push({ listener, filter: rest.length > 0 ? rest[0] : 'none' }),
    },
    onMessage: { addListener: (listener) => void messages.push(listener) },
    extensionId: 'wirecatch-id',
    firefox,
  };
  return { api, responses, removed, updated, messages };
}

function setup(firefox = false) {
  const fake = fakeApi(firefox);
  const registry = new MediaRegistry();
  const watcher = new MediaWatcher(registry, { isPrivateTab: () => Promise.resolve(false) });
  const media = new MediaCenter({
    registry,
    getTab: () => Promise.resolve(null),
    send: () => Promise.resolve({ ok: true, message: '' }),
  });
  registerMediaListeners(fake.api, { watcher, media });
  return { ...fake, registry };
}

function matches(filter: RequestFilter, url: string): boolean {
  return filter.urls.some((pattern) => new MatchPattern(pattern).includes(url));
}

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
    const { responses } = setup();
    for (const r of responses) expect(r.extra).not.toContain('blocking');
  });

  it('feeds every observed response to the watcher', async () => {
    const { responses, registry } = setup();
    responses[1]?.listener({
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
