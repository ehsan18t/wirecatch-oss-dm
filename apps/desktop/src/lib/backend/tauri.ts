// Talks to the Rust side through Tauri commands and the single `wirecatch://event` stream.
// Argument keys are camelCase here; Tauri maps them to the snake_case Rust parameter names.

import { PRODUCT_NAME } from '@wirecatch/product';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { open, save } from '@tauri-apps/plugin-dialog';
import type {
  AddonInfo,
  AddRequest,
  AppSettings,
  AppSettingsPatch,
  CaptureConfirmation,
  DownloadId,
  DownloadSummary,
  ErrorCode,
  ImportPreview,
  MediaChoice,
  MediaSelection,
  PendingCapture,
  Queue,
  QueueId,
  UiEvent,
} from '@wirecatch/protocol';
import {
  BackendError,
  COMMANDS,
  EVENT_NAME,
  SIGNAL_EVENTS,
  type AddonRequestItem,
  type DownloadWindowBackend,
  type Question,
  type ShellSignal,
  type Unsubscribe,
} from './types';

/** What the file dialogs offer for settings exports. */
const SETTINGS_FILES = { name: `${PRODUCT_NAME} settings`, extensions: ['json'] };

type Invoke = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
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

  async #call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
    try {
      return await this.#invoke<T>(command, args);
    } catch (err) {
      throw toBackendError(err);
    }
  }

  /** For commands that answer with nothing. */
  async #exec(command: string, args?: Record<string, unknown>): Promise<void> {
    await this.#call<unknown>(command, args);
  }

  listDownloads() {
    return this.#call<DownloadSummary[]>(COMMANDS.listDownloads);
  }
  addDownload(request: AddRequest) {
    return this.#call<DownloadId>(COMMANDS.addDownload, { request });
  }
  pause(id: DownloadId) {
    return this.#exec(COMMANDS.pause, { id });
  }
  resume(id: DownloadId) {
    return this.#exec(COMMANDS.resume, { id });
  }
  cancel(id: DownloadId) {
    return this.#exec(COMMANDS.cancel, { id });
  }
  remove(ids: DownloadId[], deleteFile: boolean) {
    return this.#exec(COMMANDS.remove, { ids, deleteFile });
  }
  retry(id: DownloadId) {
    return this.#exec(COMMANDS.retry, { id });
  }
  refreshLink(id: DownloadId, url: string) {
    return this.#exec(COMMANDS.refreshLink, { id, url });
  }
  keepOriginal(id: DownloadId) {
    return this.#exec(COMMANDS.keepOriginal, { id });
  }
  moveToQueue(ids: DownloadId[], queue: QueueId) {
    return this.#exec(COMMANDS.moveToQueue, { ids, queue });
  }
  reorder(queue: QueueId, idsInOrder: DownloadId[]) {
    return this.#exec(COMMANDS.reorder, { queue, idsInOrder });
  }
  pauseAll() {
    return this.#exec(COMMANDS.pauseAll);
  }
  resumeAll() {
    return this.#exec(COMMANDS.resumeAll);
  }
  listQueues() {
    return this.#call<Queue[]>(COMMANDS.listQueues);
  }
  createQueue(name: string) {
    return this.#call<Queue>(COMMANDS.createQueue, { name });
  }
  updateQueue(queue: Queue) {
    return this.#exec(COMMANDS.updateQueue, { queue });
  }
  setQueuePaused(id: QueueId, paused: boolean) {
    return this.#exec(COMMANDS.setQueuePaused, { id, paused });
  }
  startQueueNow(id: QueueId) {
    return this.#exec(COMMANDS.startQueueNow, { id });
  }
  deleteQueue(id: QueueId) {
    return this.#exec(COMMANDS.deleteQueue, { id });
  }
  getSettings() {
    return this.#call<AppSettings>(COMMANDS.getSettings);
  }
  changeSettings(patch: AppSettingsPatch) {
    return this.#exec(COMMANDS.changeSettings, { patch });
  }
  exportSettings(path: string) {
    return this.#exec(COMMANDS.exportSettings, { path });
  }
  previewImport(path: string) {
    return this.#call<ImportPreview>(COMMANDS.previewImport, { path });
  }
  importSettings(path: string) {
    return this.#exec(COMMANDS.importSettings, { path });
  }
  pendingConfirmations() {
    return this.#call<PendingCapture[]>(COMMANDS.pendingConfirmations);
  }
  confirmCapture(id: number, confirmation: CaptureConfirmation) {
    return this.#call<DownloadId>(COMMANDS.confirmCapture, { id, confirmation });
  }
  rejectCapture(id: number) {
    return this.#exec(COMMANDS.rejectCapture, { id });
  }
  getQuestion(id: number) {
    return this.#call<Question | null>(COMMANDS.getQuestion, { id });
  }
  getDownload(id: DownloadId) {
    return this.#call<DownloadSummary>(COMMANDS.getDownload, { id });
  }
  earlierDownload(id: number) {
    return this.#call<DownloadSummary | null>(COMMANDS.earlierDownload, { id });
  }
  showQuestionWindows() {
    return this.#exec(COMMANDS.showQuestionWindows);
  }
  showProgressWindow(id: DownloadId) {
    return this.#exec(COMMANDS.showProgressWindow, { id });
  }
  closeDownloadWindow() {
    return this.#exec(COMMANDS.closeDownloadWindow);
  }
  mainWindowReady() {
    return this.#exec(COMMANDS.mainWindowReady);
  }
  fitDownloadWindow(height: number) {
    return this.#exec(COMMANDS.fitDownloadWindow, { height });
  }
  openFile(id: DownloadId, allowProgram = false) {
    return this.#exec(COMMANDS.openFile, { id, allowProgram });
  }
  isProgram(id: DownloadId) {
    return this.#call<boolean>(COMMANDS.isProgram, { id });
  }
  openFolder(id: DownloadId) {
    return this.#exec(COMMANDS.openFolder, { id });
  }
  setDownloadFolder(id: DownloadId, folder: string) {
    return this.#exec(COMMANDS.setDownloadFolder, { id, folder });
  }

  takeLaunchLinks() {
    return this.#call<string[]>(COMMANDS.takeLaunchLinks);
  }
  chooseMedia(id: number, selection: MediaSelection) {
    return this.#call<DownloadId>(COMMANDS.chooseMedia, { id, selection });
  }
  dismissMediaChoice(id: number) {
    return this.#exec(COMMANDS.dismissMediaChoice, { id });
  }
  pendingMediaChoices() {
    return this.#call<MediaChoice[]>(COMMANDS.pendingMediaChoices);
  }
  pendingAddonRequests() {
    return this.#call<AddonRequestItem[]>(COMMANDS.pendingAddonRequests);
  }
  listAddons() {
    return this.#call<AddonInfo[]>(COMMANDS.listAddons);
  }
  installAddon(id: string) {
    return this.#exec(COMMANDS.installAddon, { id });
  }
  updateAddon(id: string) {
    return this.#exec(COMMANDS.updateAddon, { id });
  }
  removeAddon(id: string) {
    return this.#exec(COMMANDS.removeAddon, { id });
  }
  cancelAddonOperation(id: string) {
    return this.#exec(COMMANDS.cancelAddonOperation, { id });
  }
  checkAddonUpdates() {
    return this.#call<AddonInfo[]>(COMMANDS.checkAddonUpdates);
  }

  async subscribe(listener: (event: UiEvent) => void): Promise<Unsubscribe> {
    return this.#listen<UiEvent>(EVENT_NAME, (e) => listener(e.payload));
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
