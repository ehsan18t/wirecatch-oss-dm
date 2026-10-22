import { existsSync } from 'node:fs';
import { resolve } from 'node:path';
import {
  DOWNLOAD_WINDOW_PAGE,
  SYSTEM_APPEARANCE_EVENT,
  UI_EVENT,
  WINDOW_FRAME_EVENT,
  downloadId,
  questionId,
  queueId,
  type WindowFrame,
} from '@wirecatch/protocol';
import { describe, expect, it, vi } from 'vitest';
import { SIGNAL_EVENTS, TauriBackend, toBackendError, UNREADABLE_RELOAD_MS } from './tauri';
import { BackendError } from './types';

/** What the shell answers each command with in these tests: a new download's id, or nothing. */
const NEW_DOWNLOAD = 40;
const ANSWERS: Readonly<Record<string, unknown>> = {
  add_download: NEW_DOWNLOAD,
  confirm_capture: NEW_DOWNLOAD,
  choose_media: NEW_DOWNLOAD,
};

describe('TauriBackend', () => {
  it('calls each command with its contract arguments in one args object', async () => {
    const invoke = vi.fn((command: string) => Promise.resolve(ANSWERS[command] ?? null));
    const b = new TauriBackend(invoke);
    await b.remove([downloadId(1), downloadId(2)], true);
    await b.reorder(queueId(3), [downloadId(2), downloadId(1)]);
    await b.addDownload({
      url: 'https://x',
      queue: null,
      folder: null,
      file_name: null,
      start: true,
    });
    await b.confirmCapture(questionId(9), {
      folder: null,
      file_name: 'a',
      queue: null,
      later: true,
    });
    await b.pauseAll();
    await b.chooseMedia(questionId(4), {
      option_id: 'hd',
      folder: null,
      file_name: null,
      queue: null,
      later: false,
      output: 'mp4',
      audio_kbps: null,
      subtitles: ['sub:en'],
      subtitle_files: false,
      chapters: true,
      every_video: null,
    });
    await b.keepOriginal(downloadId(7));
    await b.installAddon('media');
    await b.getQuestion(questionId(5));
    await b.earlierDownload(questionId(5));
    await b.showQuestionWindows();
    await b.showProgressWindow(downloadId(6));
    await b.closeDownloadWindow();
    await b.fitDownloadWindow(432);
    await b.mainWindowReady();
    await b.controlWindow('toggle_maximize');
    await b.setMaximizeButton({ x: 1000, y: 0, width: 46, height: 32 });
    await b.setMaximizeButton(null);
    expect(invoke.mock.calls).toEqual([
      ['remove', { args: { ids: [1, 2], delete_file: true } }],
      ['reorder', { args: { queue: 3, ids_in_order: [2, 1] } }],
      [
        'add_download',
        {
          args: {
            request: { url: 'https://x', queue: null, folder: null, file_name: null, start: true },
          },
        },
      ],
      [
        'confirm_capture',
        {
          args: { id: 9, confirmation: { folder: null, file_name: 'a', queue: null, later: true } },
        },
      ],
      ['pause_all', undefined],
      [
        'choose_media',
        {
          args: {
            id: 4,
            selection: {
              option_id: 'hd',
              folder: null,
              file_name: null,
              queue: null,
              later: false,
              output: 'mp4',
              audio_kbps: null,
              subtitles: ['sub:en'],
              subtitle_files: false,
              chapters: true,
              every_video: null,
            },
          },
        },
      ],
      ['keep_original', { args: { id: 7 } }],
      ['install_addon', { args: { id: 'media' } }],
      ['get_question', { args: { id: 5 } }],
      ['earlier_download', { args: { id: 5 } }],
      ['show_question_windows', undefined],
      ['show_progress_window', { args: { id: 6 } }],
      ['close_download_window', undefined],
      ['fit_download_window', { args: { height: 432 } }],
      ['main_window_ready', undefined],
      ['control_window', { args: { action: 'toggle_maximize' } }],
      ['set_maximize_button', { args: { area: { x: 1000, y: 0, width: 46, height: 32 } } }],
      ['set_maximize_button', { args: { area: null } }],
    ]);
  });

  it("refuses an answer that does not have the contract's shape", async () => {
    const b = new TauriBackend(vi.fn().mockResolvedValue({ id: 'not a number' }));
    await expect(
      b.addDownload({ url: 'https://x', queue: null, folder: null, file_name: null, start: true }),
    ).rejects.toThrow(/cannot read/);
    const empty = new TauriBackend(vi.fn().mockResolvedValue(undefined));
    await expect(empty.listDownloads()).rejects.toBeInstanceOf(BackendError);
  });

  it('turns rejections into readable errors', async () => {
    // Tauri rejects with the command's error as a plain string, not an Error.
    const invoke = vi
      .fn()
      .mockRejectedValue('Wirecatch is not connected to its download engine yet.');
    const b = new TauriBackend(invoke);
    await expect(b.listDownloads()).rejects.toBeInstanceOf(BackendError);
    await expect(b.listDownloads()).rejects.toThrow(/not connected/);
    expect(toBackendError({ message: 'x' }).message).toBe('x');
    expect(toBackendError({ message: 'x' }).code).toBeNull();
    const coded = toBackendError({ code: 'addon_required', message: 'This needs the add-on.' });
    expect([coded.code, coded.message]).toEqual(['addon_required', 'This needs the add-on.']);
    expect(toBackendError({ code: 'from_the_future', message: 'x' }).code).toBeNull();
    expect(toBackendError(42).message).toMatch(/Try again/);
  });
});

describe('app events', () => {
  /** A Tauri `listen` that keeps each handler, so a test can send events through it. */
  function fakeListen() {
    const handlers = new Map<string, (event: { payload: unknown }) => void>();
    const listen = vi.fn((name: string, handler: (event: { payload: unknown }) => void) => {
      handlers.set(name, handler);
      return Promise.resolve(() => handlers.delete(name));
    });
    const send = (name: string, payload: unknown) => {
      handlers.get(name)?.({ payload });
    };
    return { listen, send };
  }

  it('passes on only events with the contract shape, and has the subscriber reload and the shell log otherwise', async () => {
    const invoke = vi.fn(() => Promise.resolve(null));
    const { listen, send } = fakeListen();
    const b = new TauriBackend(invoke, listen);
    const events = vi.fn();
    const reload = vi.fn();
    await b.subscribe(events, reload);

    send(UI_EVENT, { type: 'downloads_removed', data: [downloadId(1)] });
    expect(events).toHaveBeenCalledExactlyOnceWith({
      type: 'downloads_removed',
      data: [downloadId(1)],
    });

    send(UI_EVENT, { type: 'downloads_changed', data: [{ id: 1 }] });
    send(UI_EVENT, 'not an event');
    expect(events).toHaveBeenCalledOnce();
    expect(reload).toHaveBeenCalledTimes(2);
    expect(invoke.mock.calls).toEqual([
      ['report_unreadable_event', { args: { kind: 'downloads_changed' } }],
      ['report_unreadable_event', { args: { kind: null } }],
    ]);
  });

  it('reloads for a mismatch that keeps happening at most once a minute, and logs each time', async () => {
    vi.useFakeTimers();
    try {
      const invoke = vi.fn(() => Promise.resolve(null));
      const { listen, send } = fakeListen();
      const b = new TauriBackend(invoke, listen);
      const reload = vi.fn();
      await b.subscribe(() => undefined, reload);
      const progress = { type: 'download_progress', data: {} };

      send(UI_EVENT, progress);
      send(UI_EVENT, progress);
      expect(reload).toHaveBeenCalledOnce();
      vi.advanceTimersByTime(UNREADABLE_RELOAD_MS);
      send(UI_EVENT, progress);
      expect(reload).toHaveBeenCalledTimes(2);
      expect(invoke).toHaveBeenCalledTimes(2);
    } finally {
      vi.useRealTimers();
    }
  });
});

describe('shell signals', () => {
  it('stops the listeners already started when another cannot start', async () => {
    const signals = Object.keys(SIGNAL_EVENTS).length;
    const stops: (() => void)[] = [];
    let calls = 0;
    // Every listener starts except the last one asked for.
    const listen = vi.fn(() => {
      calls += 1;
      if (calls === signals) return Promise.reject(new Error('no event system'));
      const stop = vi.fn();
      stops.push(stop);
      return Promise.resolve(stop);
    });
    const b = new TauriBackend(vi.fn(), listen);
    await expect(b.onShellSignal(() => undefined)).rejects.toThrow('no event system');
    expect(listen).toHaveBeenCalledTimes(signals);
    expect(stops).toHaveLength(signals - 1);
    for (const stop of stops) expect(stop).toHaveBeenCalledOnce();
  });
});

describe('window frames and the system look', () => {
  /** A Tauri `listen` that remembers each listener, so a test can send it payloads. */
  function events() {
    const handlers = new Map<string, (e: { payload: unknown }) => void>();
    const listen = vi.fn((event: string, handler: (e: { payload: unknown }) => void) => {
      handlers.set(event, handler);
      return Promise.resolve(() => handlers.delete(event));
    });
    const send = (event: string, payload: unknown) => handlers.get(event)?.({ payload });
    return { listen, send };
  }

  it('pass on only payloads with the contract shape', async () => {
    const { listen, send } = events();
    const b = new TauriBackend(vi.fn(), listen);
    const frames: WindowFrame[] = [];
    const accents: (string | null)[] = [];
    await b.onWindowFrame((frame) => frames.push(frame));
    await b.onSystemAppearance((system) => accents.push(system.accent));
    const frame = { maximized: true, focused: false, maximize_hovered: true };
    send(WINDOW_FRAME_EVENT, frame);
    send(WINDOW_FRAME_EVENT, { maximized: 'yes' });
    send(SYSTEM_APPEARANCE_EVENT, { accent: '#0078d4' });
    send(SYSTEM_APPEARANCE_EVENT, { accent: 7 });
    expect(frames).toEqual([frame]);
    expect(accents).toEqual(['#0078d4']);
  });

  it('read the window frame and the system accent through checked commands', async () => {
    const frame = { maximized: false, focused: true, maximize_hovered: false };
    const answers: Record<string, unknown> = {
      window_frame: frame,
      system_appearance: { accent: null },
    };
    const b = new TauriBackend((command) => Promise.resolve(answers[command]));
    expect(await b.windowFrame()).toEqual(frame);
    expect(await b.systemAppearance()).toEqual({ accent: null });
    const wrong = new TauriBackend(() => Promise.resolve({ accent: 3 }));
    await expect(wrong.systemAppearance()).rejects.toThrow(/cannot read/);
  });
});

describe('download windows', () => {
  it('open a page the interface has', () => {
    // Vitest runs from apps/desktop, where Vite builds each page from its HTML file; the shell
    // opens DOWNLOAD_WINDOW_PAGE relative to the app.
    const page = resolve(process.cwd(), DOWNLOAD_WINDOW_PAGE);
    expect(existsSync(page), page).toBe(true);
  });
});
