/**
 * The popup's "Media on this page" section, rendered in jsdom and wired to the real
 * hub, media center and native client, which talk to a fake app.
 */
import { cleanup, render, screen, waitFor, within } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import axe from 'axe-core';
import { afterEach, describe, expect, it } from 'vitest';
import App from '../src/components/App.svelte';
import { Hub } from '../src/lib/hub';
import { MediaCenter, type TabInfo } from '../src/lib/media/center';
import { MediaRegistry } from '../src/lib/media/registry';
import type { BackgroundMessage, PopupRequest } from '../src/lib/messages';
import { NativeClient } from '../src/lib/native/client';
import { PopupModel } from '../src/lib/popup/model.svelte';
import { FakeApp, FakeDownloads, MemoryCache, channelPair, flush } from './fakes';

const TABS = new Map<number, TabInfo>([
  [
    1,
    { url: 'https://example.com/watch', title: 'A video', incognito: false, cookieStoreId: null },
  ],
  [2, { url: 'https://example.com/secret', title: 'Secret', incognito: true, cookieStoreId: null }],
  [
    4,
    {
      url: 'https://www.youtube.com/watch?v=abc',
      title: 'A talk',
      incognito: false,
      cookieStoreId: null,
    },
  ],
]);

interface Opened {
  app: FakeApp;
  client: NativeClient;
  registry: MediaRegistry;
  model: PopupModel;
  container: HTMLElement;
}

const opened: Opened[] = [];

afterEach(() => {
  cleanup();
  for (const o of opened.splice(0)) o.client.stop();
});

async function openPopup(
  tabId: number,
  options: {
    configure?: (app: FakeApp, registry: MediaRegistry) => void;
    shortcut?: string | null;
  } = {},
): Promise<Opened> {
  const app = new FakeApp();
  const registry = new MediaRegistry();
  options.configure?.(app, registry);
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
  const media: MediaCenter = new MediaCenter({
    registry,
    getTab: (id) => Promise.resolve(TABS.get(id) ?? null),
    send: (request) => hub.sendMedia(request),
  });
  const hub: Hub = new Hub({
    client,
    cache: new MemoryCache(),
    downloads: new FakeDownloads(),
    cookieHeader: () => Promise.resolve(null),
    hasSiteAccess: () => Promise.resolve(true),
    userAgent: null,
    media,
    connectWaitMs: 50,
    requestTimeoutMs: 50,
    withdrawTimeoutMs: 50,
  });
  await hub.start();
  await flush();
  const [popupEnd, backgroundEnd] = channelPair<PopupRequest, BackgroundMessage>();
  hub.addPopup(backgroundEnd);
  const model = new PopupModel(popupEnd);
  model.watchMedia(tabId);
  await flush();
  const { container } = render(App, {
    props: {
      model,
      currentUrl: TABS.get(tabId)?.url ?? null,
      mediaShortcut: options.shortcut ?? null,
    },
  });
  const result = { app, client, registry, model, container };
  opened.push(result);
  return result;
}

function section(): HTMLElement {
  return screen.getByRole('region', { name: 'Media on this page' });
}

async function axeViolations(container: HTMLElement): Promise<string[]> {
  const results = await axe.run(container, { rules: { 'color-contrast': { enabled: false } } });
  return results.violations.map((v) => `${v.id}: ${v.nodes.map((n) => n.html).join(' | ')}`);
}

describe('Media on this page', () => {
  it('explains the empty list, the media button and where to turn it on or off', async () => {
    const { app, model } = await openPopup(1);
    const region = section();
    expect(within(region).getByText(/No video or audio found on this page yet/)).toBeTruthy();
    expect(
      within(region).getByText(/Turn it off with "Show download button on videos" below/),
    ).toBeTruthy();

    await model.update({ show_media_button: false });
    await waitFor(() =>
      expect(
        within(section()).getByText(/Turn on "Show download button on videos" below/),
      ).toBeTruthy(),
    );
    expect(app.settings.show_media_button).toBe(false);
  });

  it('names the keyboard shortcut and the list as ways to download without the button', async () => {
    await openPopup(1, { shortcut: 'Alt+Shift+D' });
    expect(
      within(section()).getByText(
        'On a page, press Alt+Shift+D to send its video or audio to Wirecatch, or use the Download buttons in this list.',
      ),
    ).toBeTruthy();
    cleanup();
    await openPopup(1, { shortcut: null });
    expect(
      within(section()).getByText(
        "To send a page's video or audio with a key press, set a shortcut in your browser's extension shortcuts.",
      ),
    ).toBeTruthy();
  });

  it('lists detected media live, with a labelled Download button for each', async () => {
    const { registry } = await openPopup(1);
    registry.add(1, {
      kind: 'hls',
      url: 'https://cdn.example.com/v/master.m3u8',
      mime: null,
      size: null,
      audio: false,
    });
    registry.add(1, {
      kind: 'direct',
      url: 'https://cdn.example.com/a/song.mp3',
      mime: 'audio/mpeg',
      size: 3_670_016,
      audio: true,
    });
    await waitFor(() => expect(within(section()).getAllByRole('listitem')).toHaveLength(2));
    const region = section();
    expect(within(region).getByText('Video stream (HLS)')).toBeTruthy();
    expect(within(region).getByText('song.mp3, 3.5 MB')).toBeTruthy();
    expect(
      within(region).getByRole('button', { name: 'Download Video stream (HLS): cdn.example.com' }),
    ).toBeTruthy();
    expect(
      within(region).getByRole('button', { name: 'Download Audio file: song.mp3' }),
    ).toBeTruthy();
  });

  it('sends an item with the keyboard alone and says it was sent', async () => {
    const { app } = await openPopup(1, {
      configure: (_, registry) =>
        registry.add(1, {
          kind: 'dash',
          url: 'https://cdn.example.com/d/manifest.mpd',
          mime: null,
          size: null,
          audio: false,
        }),
    });
    const button = await within(section()).findByRole('button', {
      name: /^Download Video stream \(DASH\)/,
    });
    const user = userEvent.setup();
    // Reach the button with Tab from the top of the popup.
    for (let i = 0; i < 10 && document.activeElement !== button; i += 1) await user.tab();
    expect(document.activeElement).toBe(button);
    await user.keyboard('{Enter}');
    await waitFor(() =>
      expect(within(section()).getByRole('status').textContent).toBe('Sent to Wirecatch.'),
    );
    const call = app.calls('add_download')[0];
    expect(call?.method === 'add_download' && call.params.media).toEqual({
      kind: 'dash',
      title: 'A video',
    });
    expect(call?.method === 'add_download' && call.params.source).toBe('media_button');
  });

  it('says to open Wirecatch when it cannot be reached, instead of staying silent', async () => {
    const { app } = await openPopup(1, {
      configure: (_, registry) =>
        registry.add(1, {
          kind: 'hls',
          url: 'https://cdn.example.com/v/master.m3u8',
          mime: null,
          size: null,
          audio: false,
        }),
    });
    app.hostMissing = 'Specified native messaging host not found.';
    app.openPort?.browserDisconnect('Specified native messaging host not found.');
    await flush();
    const user = userEvent.setup();
    await user.click(within(section()).getByRole('button', { name: /^Download/ }));
    await waitFor(() =>
      expect(within(section()).getByRole('status').textContent).toBe(
        'Wirecatch could not be reached. Open Wirecatch, then try again.',
      ),
    );
    expect(app.calls('add_download')).toEqual([]);
  });

  it('offers the page on a well-known video site, and explains private windows', async () => {
    await openPopup(4);
    expect(await within(section()).findByText('This YouTube page')).toBeTruthy();
    cleanup();
    await openPopup(2);
    expect(
      await within(section()).findByText('Wirecatch does not take media from private windows.'),
    ).toBeTruthy();
  });

  it('has no accessibility problems with an empty list or a full one', async () => {
    const empty = await openPopup(1);
    await waitFor(() => expect(empty.model.media).not.toBeNull());
    expect(await axeViolations(empty.container)).toEqual([]);
    cleanup();
    const full = await openPopup(4, {
      configure: (_, registry) =>
        registry.add(4, {
          kind: 'direct',
          url: 'https://cdn.example.com/a/clip.mp4',
          mime: 'video/mp4',
          size: 1024,
          audio: false,
        }),
    });
    await within(section()).findByText('This YouTube page');
    await full.model.downloadMedia('direct:https://cdn.example.com/a/clip.mp4');
    expect(await axeViolations(full.container)).toEqual([]);
  });
});
