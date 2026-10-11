// The media quality picker, the media add-on and shell signals, against the mock backend.

import type { UiEvent } from '@wirecatch/protocol';
import { describe, expect, it, vi } from 'vitest';
import { MockBackend } from '../backend/mock';
import type { ShellSignal } from '../backend/types';
import { AppState } from './app.svelte';

const NOON = new Date(2026, 9, 10, 12, 0).getTime();

async function make() {
  const backend = new MockBackend({ autoTick: false, now: () => NOON, empty: true });
  const app = new AppState(backend);
  await app.init();
  // The add-ons load in the background after the list.
  await vi.waitFor(() => expect(app.addons).not.toBeNull());
  return { app, backend };
}

describe('media and add-ons', () => {
  it('lists one media add-on with plain words and its size', async () => {
    const { app } = await make();
    expect(app.addons).toHaveLength(1);
    const addon = app.addons?.[0];
    expect(addon?.id).toBe('media');
    expect(addon?.name).toBe('Media add-on');
    expect(addon?.description).toMatch(/YouTube and about 1,800 other sites/);
    expect(addon?.size).toBeGreaterThan(0);
    app.dispose();
  });

  it('offers the add-on when a site needs it, installs it, and the download carries on', async () => {
    const { app, backend } = await make();
    backend.simulateAddonRequired();
    expect(app.addonRequest?.addon.id).toBe('media');
    expect(app.addonRequest?.url).toMatch(/youtube/);

    await app.installAddon('media');
    expect(app.addonRequest?.addon.state.type).toBe('installing');
    backend.tick(2000);
    expect(app.addonRequest?.addon.state.type).toBe('installing');
    backend.tick(2500);
    // Installed: the prompt closes and the waiting page turns into a quality choice.
    expect(app.addonRequest).toBeNull();
    await vi.waitFor(() => expect(app.mediaChoices).toHaveLength(1));
    expect(app.addons?.[0]?.state.type).toBe('installed');
    app.dispose();
  });

  it('downloads the picked quality, joins it, then finishes', async () => {
    const { app, backend } = await make();
    backend.simulateMediaChoice();
    const choice = app.mediaChoices[0];
    expect(choice?.options[0]?.label).toMatch(/4K/);
    const best = choice?.options.find((o) => o.needs_ffmpeg);
    expect(best).toBeDefined();
    const ok = await app.chooseMedia(choice?.id ?? 0, {
      option_id: best?.id ?? '',
      file_name: null,
      folder: null,
      queue: null,
    });
    expect(ok).toBe(true);
    expect(app.mediaChoices).toHaveLength(0);
    const id = app.downloads.order[0] ?? 0;
    expect(app.downloads.get(id)?.media?.quality).toBe('4K (2160p)');

    const seen = new Set<string>();
    for (let i = 0; i < 2000 && app.downloads.get(id)?.status !== 'completed'; i++) {
      backend.tick(5000);
      seen.add(app.downloads.get(id)?.status ?? '');
    }
    expect(seen).toContain('processing');
    expect(app.downloads.get(id)?.status).toBe('completed');
    app.dispose();
  });

  it('drops a dismissed choice at once', async () => {
    const { app, backend } = await make();
    backend.simulateMediaChoice();
    backend.simulateMediaChoice();
    expect(app.mediaChoices).toHaveLength(2);
    await app.dismissMediaChoice(app.mediaChoices[0]?.id ?? 0);
    expect(app.mediaChoices).toHaveLength(1);
    app.dispose();
  });
});

describe('questions asked while no window was open', () => {
  it('come back in a new window', async () => {
    const backend = new MockBackend({ autoTick: false, now: () => NOON, empty: true });
    // Asked while closed to the tray: nobody is listening.
    backend.simulateMediaChoice();
    backend.simulateAddonRequired();

    const app = new AppState(backend);
    await app.init();
    await vi.waitFor(() => expect(app.mediaChoices).toHaveLength(1));
    await vi.waitFor(() => expect(app.addonRequest?.addon.id).toBe('media'));

    // A reload restores the open choice without doubling it; the add-on prompt is handed
    // over once.
    await app.init();
    await vi.waitFor(() => expect(app.mediaChoices).toHaveLength(1));
    app.dispose();
  });
});

describe('shell signals', () => {
  it('opens the Add dialog with links from a second launch, and reloads after missed events', async () => {
    const backend = new MockBackend({ autoTick: false, now: () => NOON, empty: true });
    let signal: ((s: ShellSignal) => void) | undefined;
    vi.spyOn(backend, 'onShellSignal').mockImplementation(async (listener) => {
      signal = listener;
      return () => {};
    });
    const links = vi
      .spyOn(backend, 'takeLaunchLinks')
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce(['https://example.com/a.zip', 'https://example.com/b.zip']);
    const app = new AppState(backend);
    await app.init();
    await vi.waitFor(() => expect(links).toHaveBeenCalledTimes(1));
    expect(app.dialog).toBeNull();

    signal?.('links');
    await vi.waitFor(() =>
      expect(app.dialog).toEqual({
        kind: 'add',
        urls: ['https://example.com/a.zip', 'https://example.com/b.zip'],
      }),
    );

    const list = vi.spyOn(backend, 'listDownloads');
    signal?.('resync');
    await vi.waitFor(() => expect(list).toHaveBeenCalled());
    app.dispose();
  });

  it('shows a storage failure from the core', async () => {
    const backend = new MockBackend({ autoTick: false, now: () => NOON, empty: true });
    let push: ((e: UiEvent) => void) | undefined;
    vi.spyOn(backend, 'subscribe').mockImplementation(async (listener) => {
      push = listener;
      return () => {};
    });
    const app = new AppState(backend);
    await app.init();
    push?.({ type: 'storage_failed', data: { message: 'Could not save.', fix: null } });
    expect(app.toasts.at(-1)).toMatchObject({ kind: 'error', message: 'Could not save.' });
    app.dispose();
  });
});
