import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it, vi } from 'vitest';
import { TauriBackend, toBackendError } from './tauri';
import { BackendError, COMMANDS } from './types';

// The command names the core team implements. They must not drift.
const FIXED = [
  'list_downloads',
  'add_download',
  'pause',
  'resume',
  'cancel',
  'remove',
  'retry',
  'refresh_link',
  'move_to_queue',
  'reorder',
  'pause_all',
  'resume_all',
  'list_queues',
  'create_queue',
  'update_queue',
  'delete_queue',
  'set_queue_paused',
  'start_queue_now',
  'get_settings',
  'update_settings',
  'export_settings',
  'preview_import',
  'import_settings',
  'pending_confirmations',
  'confirm_capture',
  'reject_capture',
  'open_file',
  'open_folder',
  'set_download_folder',
  'is_program',
  'take_launch_links',
  'choose_media',
  'dismiss_media_choice',
  'pending_media_choices',
  'pending_addon_requests',
  'list_addons',
  'install_addon',
  'update_addon',
  'remove_addon',
  'cancel_addon_operation',
  'check_addon_updates',
  'get_question',
  'get_download',
  'earlier_download',
  'show_question_windows',
  'show_progress_window',
  'close_download_window',
  'fit_download_window',
];

describe('Tauri command names', () => {
  it('match the agreed list exactly', () => {
    expect(Object.values(COMMANDS).sort()).toEqual([...FIXED].sort());
  });

  it('are all registered on the Rust side', () => {
    // Vitest runs from apps/desktop.
    const lib = readFileSync(resolve(process.cwd(), 'src-tauri/src/lib.rs'), 'utf8');
    const handler = /generate_handler!\[([\s\S]*?)\]/.exec(lib)?.[1] ?? '';
    const registered = handler
      .split(',')
      // Registered as `commands::name`.
      .map((s) => s.trim().replace(/^commands::/, ''))
      .filter(Boolean);
    expect(registered.sort()).toEqual([...FIXED].sort());
  });

  it('are all in the contract table, and none is pending', () => {
    const lib = readFileSync(resolve(process.cwd(), 'src-tauri/src/lib.rs'), 'utf8');
    const rows = lib.split('\n').filter((line) => /^\/\/! \| `[a-z_]+` \|/.test(line));
    const documented = rows.map((line) => /`([a-z_]+)`/.exec(line)?.[1] ?? '');
    expect(documented.sort()).toEqual([...FIXED].sort());
    // A stand-in was marked "(pending, WP11)".
    for (const row of rows) expect(row, row).not.toMatch(/\(pending|WP11/);
  });
});

describe('TauriBackend', () => {
  it('calls each command with camelCase argument keys', async () => {
    const invoke = vi.fn(async () => undefined);
    const b = new TauriBackend(invoke as never);
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
    });
    await b.installAddon('media');
    await b.getQuestion(5);
    await b.earlierDownload(5);
    await b.getDownload(6);
    await b.showQuestionWindows();
    await b.showProgressWindow(6);
    await b.closeDownloadWindow();
    await b.fitDownloadWindow(432);
    expect(invoke.mock.calls).toEqual([
      ['remove', { ids: [1, 2], deleteFile: true }],
      ['reorder', { queue: 3, idsInOrder: [2, 1] }],
      [
        'add_download',
        { request: { url: 'https://x', queue: null, folder: null, file_name: null, start: true } },
      ],
      [
        'confirm_capture',
        { id: 9, confirmation: { folder: null, file_name: 'a', queue: null, later: true } },
      ],
      ['pause_all', undefined],
      [
        'choose_media',
        {
          id: 4,
          selection: { option_id: 'hd', folder: null, file_name: null, queue: null, later: false },
        },
      ],
      ['install_addon', { id: 'media' }],
      ['get_question', { id: 5 }],
      ['earlier_download', { id: 5 }],
      ['get_download', { id: 6 }],
      ['show_question_windows', undefined],
      ['show_progress_window', { id: 6 }],
      ['close_download_window', undefined],
      ['fit_download_window', { height: 432 }],
    ]);
  });

  it('turns rejections into readable errors', async () => {
    const b = new TauriBackend((async () => {
      throw 'Wirecatch is not connected to its download engine yet.';
    }) as never);
    await expect(b.listDownloads()).rejects.toBeInstanceOf(BackendError);
    await expect(b.listDownloads()).rejects.toThrow(/not connected/);
    expect(toBackendError({ message: 'x' }).message).toBe('x');
    expect(toBackendError(42).message).toMatch(/Try again/);
  });
});
