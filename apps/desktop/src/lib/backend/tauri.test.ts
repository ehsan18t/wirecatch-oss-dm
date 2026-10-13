import { existsSync } from 'node:fs';
import { resolve } from 'node:path';
import { DOWNLOAD_WINDOW_PAGE } from '@wirecatch/protocol';
import { describe, expect, it, vi } from 'vitest';
import { SIGNAL_EVENTS, TauriBackend, toBackendError } from './tauri';
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
    await b.remove([1, 2], true);
    await b.reorder(3, [2, 1]);
    await b.addDownload({
      url: 'https://x',
      queue: null,
      folder: null,
      file_name: null,
      start: true,
    });
    await b.confirmCapture(9, { folder: null, file_name: 'a', queue: null, later: true });
    await b.pauseAll();
    await b.chooseMedia(4, {
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
    });
    await b.keepOriginal(7);
    await b.installAddon('media');
    await b.getQuestion(5);
    await b.earlierDownload(5);
    await b.showQuestionWindows();
    await b.showProgressWindow(6);
    await b.closeDownloadWindow();
    await b.fitDownloadWindow(432);
    await b.mainWindowReady();
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

describe('download windows', () => {
  it('open a page the interface has', () => {
    // Vitest runs from apps/desktop; the shell opens DOWNLOAD_WINDOW_PAGE relative to the app.
    const page = resolve(process.cwd(), 'src/routes', DOWNLOAD_WINDOW_PAGE, '+page.svelte');
    expect(existsSync(page), page).toBe(true);
  });
});
