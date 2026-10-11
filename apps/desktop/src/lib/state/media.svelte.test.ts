// The media quality picker, the media add-on and shell signals, against the mock backend.

import type { UiEvent } from '@wirecatch/protocol';
import { describe, expect, it, vi } from 'vitest';
import { MockBackend } from '../backend/mock';
import type { ShellSignal } from '../backend/types';
import { buildCommands } from '../commands';
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
    expect(addon?.description).toMatch(/YouTube \(yt-dlp\)/);
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

  it('installs the add-on for a quality that needs it, then downloads, joins and finishes', async () => {
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
    // The add-on installs first; the picker steps aside for its prompt and nothing is lost.
    expect(app.choiceAwaitingAddon?.id).toBe(choice?.id);
    expect(app.question?.kind).toBe('addon');
    expect(app.openMediaChoices).toHaveLength(0);
    expect(app.addons?.[0]?.state.type).toBe('installing');
    expect(app.downloads.order).toHaveLength(0);
    backend.tick(4500);
    // Installed: the picked quality downloads by itself.
    await vi.waitFor(() => expect(app.downloads.order).toHaveLength(1));
    expect(app.mediaChoices).toHaveLength(0);
    expect(app.choiceAwaitingAddon).toBeNull();
    expect(app.question).toBeNull();
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

  it('says it is joining, and a stuck join can be paused', async () => {
    const { app, backend } = await make();
    await app.installAddon('media');
    backend.tick(4500);
    backend.simulateMediaChoice();
    const choice = app.mediaChoices[0];
    const joined = choice?.options.find((o) => o.needs_ffmpeg);
    await app.chooseMedia(choice?.id ?? 0, {
      option_id: joined?.id ?? '',
      file_name: null,
      folder: null,
      queue: null,
    });
    const id = app.downloads.order[0] ?? 0;
    for (let i = 0; i < 400 && app.downloads.get(id)?.status !== 'processing'; i++) {
      backend.tick(250);
    }
    expect(app.downloads.get(id)?.status).toBe('processing');
    expect(app.counts.joining).toBe(1);
    expect(app.counts.status.active).toBe(1);

    await app.togglePause([id]);
    expect(app.downloads.get(id)?.status).toBe('paused');
    expect(app.counts.joining).toBe(0);
    app.dispose();
  });

  it('gives the picker back when the add-on is declined, and installs nothing', async () => {
    const { app, backend } = await make();
    backend.simulateMediaChoice();
    const choice = app.mediaChoices[0];
    const joined = choice?.options.find((o) => o.needs_ffmpeg);
    // The core asks for the add-on when the window did not know it was missing.
    app.addons = null;
    const ok = await app.chooseMedia(choice?.id ?? 0, {
      option_id: joined?.id ?? '',
      file_name: null,
      folder: null,
      queue: null,
    });
    expect(ok).toBe(true);
    // No error note on top of the prompt: the prompt explains.
    expect(app.toasts.filter((t) => t.kind === 'error')).toHaveLength(0);
    expect(app.question?.kind).toBe('addon');
    app.declineAddonRequest();
    expect(app.choiceAwaitingAddon).toBeNull();
    expect(app.question).toEqual({ kind: 'media', choice, waiting: 0 });
    const [addon] = await backend.listAddons();
    expect(addon?.state.type).toBe('not_installed');
    app.dispose();
  });

  it('closes a question answered elsewhere, and says so only when it was on screen', async () => {
    const { app, backend } = await make();
    backend.simulateConfirmation();
    backend.simulateMediaChoice();
    expect(app.question?.kind).toBe('capture');
    const capture = app.pending[0];

    // The capture on screen is answered in another window.
    backend.simulateAnsweredElsewhere();
    expect(app.pending).toHaveLength(0);
    expect(app.toasts.at(-1)?.message).toBe(
      `${capture?.file_name} was answered in another window, or the browser took it back.`,
    );
    // The quality picker is next; answered here, it closes without a note.
    expect(app.question?.kind).toBe('media');
    const notes = app.toasts.length;
    await app.dismissMediaChoice(app.mediaChoices[0]?.id ?? 0);
    expect(app.question).toBeNull();
    expect(app.toasts).toHaveLength(notes);

    // One put aside (shown in the sidebar) closes quietly too.
    backend.simulateMediaChoice();
    app.hideMediaChoices();
    backend.simulateAnsweredElsewhere();
    expect(app.mediaChoices).toHaveLength(0);
    expect(app.toasts).toHaveLength(notes);
    app.dispose();
  });

  it('shows a failure that is not about the add-on, instead of waiting for the add-on', async () => {
    const { app, backend } = await make();
    backend.simulateMediaChoice();
    const choice = app.mediaChoices[0];
    const joined = choice?.options.find((o) => o.needs_ffmpeg);
    // The window does not know the add-on is missing, and the core fails for another reason.
    app.addons = null;
    vi.spyOn(backend, 'chooseMedia').mockRejectedValueOnce(new Error('The disk is full.'));
    const ok = await app.chooseMedia(choice?.id ?? 0, {
      option_id: joined?.id ?? '',
      file_name: null,
      folder: null,
      queue: null,
    });
    expect(ok).toBe(false);
    expect(app.choiceAwaitingAddon).toBeNull();
    expect(app.toasts.at(-1)).toMatchObject({ kind: 'error', message: 'The disk is full.' });
    app.dispose();
  });

  it('says nothing when a question answered here closes', async () => {
    const { app, backend } = await make();
    backend.simulateConfirmation();
    backend.simulateConfirmation();
    backend.simulateMediaChoice();
    const [first, second] = app.pending;
    // Each answer here makes the core close the question while it is still on screen.
    await app.confirmCapture(first?.id ?? -1, { folder: null, file_name: null, queue: null });
    await app.rejectCapture(second?.id ?? -1);
    const choice = app.mediaChoices[0];
    await app.chooseMedia(choice?.id ?? -1, {
      option_id: '720',
      folder: null,
      file_name: null,
      queue: null,
    });
    expect(app.question).toBeNull();
    expect(app.toasts.filter((t) => /another window/.test(t.message))).toEqual([]);
    app.dispose();
  });

  it('Not now drops only the page its prompt is about', async () => {
    const { app, backend } = await make();
    backend.simulateAddonRequired();
    const [first] = await backend.pendingAddonRequests();
    backend.simulateAddonRequired();
    // The event carries the page's id: the prompt is about the second page.
    const [, second] = await backend.pendingAddonRequests();
    expect(app.addonRequest?.id).toBe(second?.id);
    app.declineAddonRequest();
    expect(app.question).toBeNull();
    await vi.waitFor(async () =>
      expect((await backend.pendingAddonRequests()).map((p) => p.id)).toEqual([first?.id]),
    );
    expect(app.toasts).toHaveLength(0);
    app.dispose();
  });

  it('Escape only puts the add-on prompt aside; the sidebar note brings it back', async () => {
    const { app, backend } = await make();
    backend.simulateAddonRequired();
    const asked = app.addonRequest;
    app.hideAddonRequest();
    expect(app.question).toBeNull();
    // Nothing was answered: the page still waits, and the palette offers the prompt.
    expect(await backend.pendingAddonRequests()).toHaveLength(1);
    const show = buildCommands(app).find((c) => c.id === 'addon.prompt');
    expect(show?.title).toMatch(/waits for the media add-on/);
    show?.run();
    expect(app.question).toEqual({ kind: 'addon', request: asked });
    app.dispose();
  });

  it('closes the add-on prompt when its page goes away, and says so', async () => {
    const { app, backend } = await make();
    backend.simulateAddonRequired();
    backend.simulateAnsweredElsewhere();
    expect(app.addonRequest).toBeNull();
    expect(app.toasts.at(-1)?.message).toBe(
      'The video from www.youtube.com no longer waits for the media add-on.',
    );
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

  it('puts a closed picker aside and brings it back from the palette', async () => {
    const { app, backend } = await make();
    backend.simulateMediaChoice();
    app.hideMediaChoices();
    // Closing is not an answer: the choice still waits.
    expect(app.mediaChoices).toHaveLength(1);
    expect(app.mediaHidden).toBe(true);
    const pick = buildCommands(app).find((c) => c.id === 'media.pick');
    expect(pick?.title).toMatch(/Pick a quality/);
    pick?.run();
    expect(app.mediaHidden).toBe(false);
    // A new choice always shows, even after putting one aside.
    app.hideMediaChoices();
    backend.simulateMediaChoice();
    expect(app.mediaHidden).toBe(false);
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
    expect(app.addonRequest?.url).toMatch(/youtube/);
    expect(app.addonRequest?.id).not.toBeNull();

    // A reload restores both from the core, without doubling the choice.
    const fresh = new AppState(backend);
    await fresh.init();
    await vi.waitFor(() => expect(fresh.mediaChoices).toHaveLength(1));
    await vi.waitFor(() => expect(fresh.addonRequest?.url).toMatch(/youtube/));
    await app.init();
    await vi.waitFor(() => expect(app.mediaChoices).toHaveLength(1));
    fresh.dispose();
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
    // A banner that stays, not a note that fades.
    expect(app.storageProblem).toBe('Could not save.');
    expect(app.toasts.some((t) => t.message === 'Could not save.')).toBe(false);
    await app.retryStorage();
    expect(app.storageProblem).toBeNull();
    push?.({ type: 'storage_failed', data: { message: 'Again.', fix: null } });
    app.dismissStorageProblem();
    expect(app.storageProblem).toBeNull();
    app.dispose();
  });
});
