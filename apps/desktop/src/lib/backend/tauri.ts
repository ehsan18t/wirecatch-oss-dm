// Talks to the Rust side through the commands of the generated contract (`CommandMap`, from
// `crates/protocol/src/desktop.rs`) and the single `UI_EVENT` stream. Each command takes its
// arguments as one `args` object keyed by the Rust names.

import { PRODUCT_NAME } from '@wirecatch/product';
import { getName } from '@tauri-apps/api/app';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { open, save } from '@tauri-apps/plugin-dialog';
import {
  LINKS_EVENT,
  RESYNC_EVENT,
  UI_EVENT,
  type AddonId,
  type AddRequest,
  type AppSettingsPatch,
  type CaptureConfirmation,
  type CommandMap,
  type DownloadId,
  type ErrorCode,
  type MediaSelection,
  type QuestionId,
  type Queue,
  type QueueId,
  type UiEvent,
} from '@wirecatch/protocol';
import {
  BackendError,
  type DownloadWindowBackend,
  type ShellSignal,
  type Unsubscribe,
} from './types';

/** Shell signals, as the Tauri events with no payload that carry them. */
export const SIGNAL_EVENTS: Readonly<Record<ShellSignal, string>> = {
  resync: RESYNC_EVENT,
  links: LINKS_EVENT,
};

/** What the file dialogs offer for settings exports. */
const SETTINGS_FILES = { name: `${PRODUCT_NAME} settings`, extensions: ['json'] };

type Invoke = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;

/** A command's name in the contract. */
type Command = keyof CommandMap;
/** What a command takes: nothing for one whose `args` is `null`, else its `args` object. */
type ArgsOf<C extends Command> = CommandMap[C]['args'] extends null ? [] : [CommandMap[C]['args']];
/** What a command answers with. */
type OutputOf<C extends Command> = CommandMap[C]['output'];
/** Tauri's `listen`; tests pass their own. */
type Listen = typeof listen;

/** Every error code, so a code read from the Rust side can be checked; a code left out fails the type check. */
const ERROR_CODES: Readonly<Record<ErrorCode, true>> = {
  unsupported_version: true,
  invalid_request: true,
  invalid_url: true,
  not_found: true,
  app_unavailable: true,
  app_not_running: true,
  internal: true,
  not_started: true,
  addon_required: true,
  limit_reached: true,
  wrong_state: true,
  update_check_failed: true,
};

function isErrorCode(value: unknown): value is ErrorCode {
  return typeof value === 'string' && Object.hasOwn(ERROR_CODES, value);
}

/**
 * Turns whatever the Rust side rejected with into a `BackendError` with readable text. A command
 * rejects with the protocol's `{ code, message }`; the code comes along when it is one this build
 * knows.
 */
export function toBackendError(err: unknown): BackendError {
  if (err instanceof BackendError) return err;
  if (typeof err === 'string') return new BackendError(err);
  if (err instanceof Error) return new BackendError(err.message);
  if (err && typeof err === 'object' && 'message' in err && typeof err.message === 'string') {
    const code = 'code' in err && isErrorCode(err.code) ? err.code : null;
    return new BackendError(err.message, code);
  }
  return new BackendError('Something went wrong talking to the download engine. Try again.');
}

export class TauriBackend implements DownloadWindowBackend {
  readonly kind = 'tauri';
  readonly #invoke: Invoke;
  readonly #listen: Listen;

  constructor(invoker: Invoke = invoke, listener: Listen = listen) {
    this.#invoke = invoker;
    this.#listen = listener;
  }

  /** Calls `command` with its contract arguments; the answer has the contract's type. */
  async #call<C extends Command>(command: C, ...args: ArgsOf<C>): Promise<OutputOf<C>> {
    try {
      return await this.#invoke<OutputOf<C>>(
        command,
        args.length === 0 ? undefined : { args: args[0] },
      );
    } catch (err) {
      throw toBackendError(err);
    }
  }

  /** For commands that answer with nothing. */
  async #exec<C extends Command>(command: C, ...args: ArgsOf<C>): Promise<void> {
    await this.#call(command, ...args);
  }

  listDownloads() {
    return this.#call('list_downloads');
  }
  addDownload(request: AddRequest) {
    return this.#call('add_download', { request });
  }
  pause(id: DownloadId) {
    return this.#exec('pause', { id });
  }
  resume(id: DownloadId) {
    return this.#exec('resume', { id });
  }
  cancel(id: DownloadId) {
    return this.#exec('cancel', { id });
  }
  remove(ids: DownloadId[], deleteFile: boolean) {
    return this.#exec('remove', { ids, delete_file: deleteFile });
  }
  retry(id: DownloadId) {
    return this.#exec('retry', { id });
  }
  refreshLink(id: DownloadId, url: string) {
    return this.#exec('refresh_link', { id, url });
  }
  keepOriginal(id: DownloadId) {
    return this.#exec('keep_original', { id });
  }
  moveToQueue(ids: DownloadId[], queue: QueueId) {
    return this.#exec('move_to_queue', { ids, queue });
  }
  reorder(queue: QueueId, idsInOrder: DownloadId[]) {
    return this.#exec('reorder', { queue, ids_in_order: idsInOrder });
  }
  pauseAll() {
    return this.#exec('pause_all');
  }
  resumeAll() {
    return this.#exec('resume_all');
  }
  listQueues() {
    return this.#call('list_queues');
  }
  createQueue(name: string) {
    return this.#call('create_queue', { name });
  }
  updateQueue(queue: Queue) {
    return this.#exec('update_queue', { queue });
  }
  setQueuePaused(id: QueueId, paused: boolean) {
    return this.#exec('set_queue_paused', { id, paused });
  }
  startQueueNow(id: QueueId) {
    return this.#exec('start_queue_now', { id });
  }
  deleteQueue(id: QueueId) {
    return this.#exec('delete_queue', { id });
  }
  getSettings() {
    return this.#call('get_settings');
  }
  changeSettings(patch: AppSettingsPatch) {
    return this.#exec('change_settings', { patch });
  }
  exportSettings(path: string) {
    return this.#exec('export_settings', { path });
  }
  previewImport(path: string) {
    return this.#call('preview_import', { path });
  }
  importSettings(path: string) {
    return this.#exec('import_settings', { path });
  }
  pendingConfirmations() {
    return this.#call('pending_confirmations');
  }
  confirmCapture(id: QuestionId, confirmation: CaptureConfirmation) {
    return this.#call('confirm_capture', { id, confirmation });
  }
  rejectCapture(id: QuestionId) {
    return this.#exec('reject_capture', { id });
  }
  getQuestion(id: QuestionId) {
    return this.#call('get_question', { id });
  }
  getDownload(id: DownloadId) {
    return this.#call('get_download', { id });
  }
  earlierDownload(id: QuestionId) {
    return this.#call('earlier_download', { id });
  }
  showQuestionWindows() {
    return this.#exec('show_question_windows');
  }
  showProgressWindow(id: DownloadId) {
    return this.#exec('show_progress_window', { id });
  }
  closeDownloadWindow() {
    return this.#exec('close_download_window');
  }
  mainWindowReady() {
    return this.#exec('main_window_ready');
  }
  fitDownloadWindow(height: number) {
    return this.#exec('fit_download_window', { height });
  }
  openFile(id: DownloadId, allowProgram = false) {
    return this.#exec('open_file', { id, allow_program: allowProgram });
  }
  isProgram(id: DownloadId) {
    return this.#call('is_program', { id });
  }
  openFolder(id: DownloadId) {
    return this.#exec('open_folder', { id });
  }
  setDownloadFolder(id: DownloadId, folder: string) {
    return this.#exec('set_download_folder', { id, folder });
  }

  takeLaunchLinks() {
    return this.#call('take_launch_links');
  }
  chooseMedia(id: QuestionId, selection: MediaSelection) {
    return this.#call('choose_media', { id, selection });
  }
  dismissMediaChoice(id: QuestionId) {
    return this.#exec('dismiss_media_choice', { id });
  }
  pendingMediaChoices() {
    return this.#call('pending_media_choices');
  }
  pendingAddonRequests() {
    return this.#call('pending_addon_requests');
  }
  listAddons() {
    return this.#call('list_addons');
  }
  installAddon(id: AddonId) {
    return this.#exec('install_addon', { id });
  }
  updateAddon(id: AddonId) {
    return this.#exec('update_addon', { id });
  }
  removeAddon(id: AddonId) {
    return this.#exec('remove_addon', { id });
  }
  cancelAddonOperation(id: AddonId) {
    return this.#exec('cancel_addon_operation', { id });
  }
  checkAddonUpdates() {
    return this.#call('check_addon_updates');
  }

  async subscribe(listener: (event: UiEvent) => void): Promise<Unsubscribe> {
    return this.#listen<UiEvent>(UI_EVENT, (e) => listener(e.payload));
  }

  async onShellSignal(listener: (signal: ShellSignal) => void): Promise<Unsubscribe> {
    const signals = Object.keys(SIGNAL_EVENTS) as ShellSignal[];
    const started = await Promise.allSettled(
      signals.map((signal) => this.#listen(SIGNAL_EVENTS[signal], () => listener(signal))),
    );
    const stops = started.flatMap((s) => (s.status === 'fulfilled' ? [s.value] : []));
    const failed = started.find((s) => s.status === 'rejected');
    if (failed) {
      // Nothing would ever stop the listeners that did start.
      for (const stop of stops) stop();
      throw failed.reason;
    }
    return () => {
      for (const stop of stops) stop();
    };
  }

  async pickFolder(current?: string) {
    const picked = await open({ directory: true, multiple: false, defaultPath: current });
    return typeof picked === 'string' ? picked : null;
  }
  appName() {
    return getName();
  }
  async pickSaveFile(suggestedName: string) {
    return save({
      defaultPath: suggestedName,
      filters: [SETTINGS_FILES],
    });
  }
  async pickOpenFile() {
    const picked = await open({
      multiple: false,
      directory: false,
      filters: [SETTINGS_FILES],
    });
    return typeof picked === 'string' ? picked : null;
  }
}
