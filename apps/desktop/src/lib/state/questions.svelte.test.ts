// The main window and questions (D23): it counts the downloads waiting for an answer and never
// asks them itself, so a question answered anywhere never comes back when a window opens or
// reloads. Also the media add-on list, joining video and audio, and shell signals, against the
// mock backend.

import { questionId, type UiEvent } from '@wirecatch/protocol';
import { describe, expect, it, vi } from 'vitest';
import { appState, mockBackend } from '$lib/testing/setup';
import type { ShellSignal } from '../backend';
import { buildCommands } from '../commands';

function backend() {
  return mockBackend({ empty: true });
}

async function make(b = backend()) {
  const app = appState(b);
  await app.init();
  // The add-ons load in the background after the list.
  await vi.waitFor(() => expect(app.addons.list).not.toBeNull());
  return { app, backend: b };
}

const select = (option: string, later = false) => ({
  option_id: option,
  folder: null,
  file_name: null,
  queue: null,
  later,
  output: 'original' as const,
  audio_kbps: null,
  subtitles: [],
  subtitle_files: false,
  chapters: false,
});

describe('media and add-ons', () => {
  it('lists one media add-on with plain words and its size', async () => {
    const { app } = await make();
    expect(app.addons.list).toHaveLength(1);
    const addon = app.addons.list?.[0];
    expect(addon?.id).toBe('media');
    expect(addon?.name).toBe('Media add-on');
    expect(addon?.description).toMatch(/YouTube \(yt-dlp\)/);
    expect(addon?.size).toBeGreaterThan(0);
  });

  it('says it is joining, and a stuck join can be paused', async () => {
    const { app, backend } = await make();
    await app.addons.install('media');
    backend.tick(4500);
    backend.simulateMediaChoice();
    const choice = app.questions.mediaChoices[0];
    const joined = choice?.options.find((o) => o.needs_ffmpeg);
    const id = await backend.chooseMedia(choice?.id ?? questionId(0), select(joined?.id ?? ''));
    for (let i = 0; i < 400 && app.downloads.get(id)?.status !== 'processing'; i++) {
      backend.tick(250);
    }
    expect(app.downloads.get(id)?.status).toBe('processing');
    expect(app.counts.joining).toBe(1);
    expect(app.counts.status.active).toBe(1);

    await app.actions.togglePause([id]);
    expect(app.downloads.get(id)?.status).toBe('paused');
    expect(app.counts.joining).toBe(0);
  });
});

describe('the main window only counts questions', () => {
  it('counts every kind of waiting download once and shows none as a dialog', async () => {
    const { app, backend } = await make();
    backend.simulateConfirmation();
    backend.simulateMediaChoice();
    backend.simulateAddonRequired();
    expect(app.questions.waitingCount).toBe(3);
    expect(app.dialog).toBeNull();
    // Each one opened its own window instead.
    expect(backend.windows.windows.map((w) => w.shows.kind)).toEqual([
      'question',
      'question',
      'question',
    ]);
    // A quality picked that needs the add-on is still the one question.
    backend.simulateMediaChoice();
    const choice = app.questions.mediaChoices[1];
    await expect(
      backend.chooseMedia(choice?.id ?? questionId(0), select('2160')),
    ).rejects.toThrow();
    expect(app.questions.waitingCount).toBe(4);

    // Answered or taken back anywhere: no longer counted.
    await backend.rejectCapture(app.questions.pending[0]?.id ?? questionId(0));
    backend.simulateAnsweredElsewhere();
    expect(app.questions.waitingCount).toBe(2);
  });

  it('offers the waiting windows from the sidebar entry and the palette', async () => {
    const { app, backend } = await make();
    backend.simulateConfirmation();
    const [window] = backend.windows.windows;
    backend.windows.gone(window?.label ?? '');
    expect(backend.windows.windows).toHaveLength(0);
    const show = buildCommands(app).find((c) => c.id === 'questions.show');
    expect(show?.title).toBe('Show the download that is waiting for you');
    await show?.run();
    expect(backend.windows.windows.map((w) => w.label)).toEqual([window?.label]);
  });

  it('opens a progress window for any download, once', async () => {
    const { app, backend } = await make();
    const id = await backend.addDownload({
      url: 'https://example.com/a.zip',
      queue: null,
      folder: null,
      file_name: null,
      start: true,
    });
    // Added from inside the app: no window of its own.
    expect(backend.windows.windows).toHaveLength(0);
    await app.showProgressWindow(id);
    await app.showProgressWindow(id);
    expect(backend.windows.windows.map((w) => w.shows)).toEqual([{ kind: 'download', id }]);
  });
});

describe('a question answered anywhere never comes back', () => {
  // The maintainer saw the same "Pick a quality" each time the main window was opened again.
  it('a picked quality is not asked again by a window opened later', async () => {
    const b = backend();
    const first = await make(b);
    b.simulateMediaChoice();
    const choice = first.app.questions.mediaChoices[0];
    await b.chooseMedia(choice?.id ?? questionId(0), select('720'));
    first.app.dispose();

    // The main window is closed and opened again: a fresh state loads from the core.
    const again = await make(b);
    await vi.waitFor(() => expect(again.app.ready).toBe(true));
    await new Promise((r) => setTimeout(r, 0));
    expect(again.app.questions.mediaChoices).toEqual([]);
    expect(again.app.questions.waitingCount).toBe(0);
    expect(again.app.dialog).toBeNull();
    expect(await b.getQuestion(choice?.id ?? questionId(0))).toBeNull();
  });

  it('a confirmed file is not asked again by a window opened later', async () => {
    const b = backend();
    b.simulateConfirmation();
    const [asked] = await b.pendingConfirmations();
    await b.confirmCapture(asked?.id ?? questionId(0), {
      folder: null,
      file_name: null,
      queue: null,
      later: false,
    });
    const { app } = await make(b);
    expect(app.questions.pending).toEqual([]);
    expect(app.questions.waitingCount).toBe(0);
    expect(app.dialog).toBeNull();
  });

  it('a question put aside is counted, never shown, and not opened again by itself', async () => {
    const b = backend();
    b.simulateConfirmation();
    const [window] = b.windows.windows;
    // The person closes its window: put aside, not answered.
    b.windows.gone(window?.label ?? '');
    const { app } = await make(b);
    expect(app.questions.waitingCount).toBe(1);
    expect(app.dialog).toBeNull();
    // Loading the main window opens no question window.
    expect(b.windows.windows).toHaveLength(0);
    // A reload counts it once, not twice.
    await app.init();
    await vi.waitFor(() => expect(app.questions.waitingCount).toBe(1));
  });
});

describe('questions asked while no window was open', () => {
  it('are counted in a new window, without doubling on reload', async () => {
    const b = backend();
    // Asked while closed to the tray: nobody is listening.
    b.simulateMediaChoice();
    b.simulateAddonRequired();

    const { app } = await make(b);
    await vi.waitFor(() => expect(app.questions.waitingCount).toBe(2));
    await app.init();
    await vi.waitFor(() => expect(app.questions.mediaChoices).toHaveLength(1));
    expect(app.questions.waitingCount).toBe(2);
  });
});

describe('shell signals', () => {
  it('opens the Add dialog with links from a second launch, and reloads after missed events', async () => {
    const b = backend();
    let signal: ((s: ShellSignal) => void) | undefined;
    vi.spyOn(b, 'onShellSignal').mockImplementation((listener) => {
      signal = listener;
      return Promise.resolve(vi.fn());
    });
    const links = vi
      .spyOn(b, 'takeLaunchLinks')
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce(['https://example.com/a.zip', 'https://example.com/b.zip']);
    const app = appState(b);
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

    const list = vi.spyOn(b, 'listDownloads');
    signal?.('resync');
    await vi.waitFor(() => expect(list).toHaveBeenCalled());
  });

  it('shows a storage failure from the core', async () => {
    const b = backend();
    let push: ((e: UiEvent) => void) | undefined;
    vi.spyOn(b, 'subscribe').mockImplementation((listener) => {
      push = listener;
      return Promise.resolve(vi.fn());
    });
    const app = appState(b);
    await app.init();
    push?.({ type: 'storage_failed', data: { message: 'Could not save.', fix: null } });
    // A banner that stays, not a note that fades.
    expect(app.storageProblem).toBe('Could not save.');
    expect(app.toasts.items.some((t) => t.message === 'Could not save.')).toBe(false);
    await app.retryStorage();
    expect(app.storageProblem).toBeNull();
    push?.({ type: 'storage_failed', data: { message: 'Again.', fix: null } });
    app.dismissStorageProblem();
    expect(app.storageProblem).toBeNull();
  });
});
