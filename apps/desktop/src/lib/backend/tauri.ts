// Talks to the Rust side through Tauri commands and the single `wirecatch://event` stream.
// Argument keys are camelCase here; Tauri maps them to the snake_case Rust parameter names.

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { open, save } from '@tauri-apps/plugin-dialog';
import type {
  AddonInfo,
  AddRequest,
  AppSettings,
  CaptureConfirmation,
  DownloadId,
  DownloadSummary,
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
  type Backend,
  type Question,
  type ShellSignal,
  type Unsubscribe,
} from './types';

type Invoke = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;

/** Turns whatever the Rust side rejected with into a `BackendError` with readable text. */
export function toBackendError(err: unknown): BackendError {
  if (err instanceof BackendError) return err;
  if (typeof err === 'string') return new BackendError(err);
  if (err instanceof Error) return new BackendError(err.message);
  if (err && typeof err === 'object' && 'message' in err && typeof err.message === 'string') {
    return new BackendError(err.message);
  }
  return new BackendError('Something went wrong talking to the download engine. Try again.');
}

export class TauriBackend implements Backend {
  readonly kind = 'tauri';
  readonly #invoke: Invoke;

  constructor(invoker: Invoke = invoke) {
    this.#invoke = invoker;
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
  updateSettings(settings: AppSettings) {
    return this.#exec(COMMANDS.updateSettings, { settings });
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
  /** Closes the download window this page runs in (only a download window can). */
  closeDownloadWindow() {
    return this.#exec(COMMANDS.closeDownloadWindow);
  }
  mainWindowReady() {
    return this.#exec(COMMANDS.mainWindowReady);
  }
  /** Makes the download window this page runs in as tall as its content (logical pixels). */
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
    return listen<UiEvent>(EVENT_NAME, (e) => listener(e.payload));
  }

  async onShellSignal(listener: (signal: ShellSignal) => void): Promise<Unsubscribe> {
    const signals = Object.keys(SIGNAL_EVENTS) as ShellSignal[];
    const stops = await Promise.all(
      signals.map((signal) => listen(SIGNAL_EVENTS[signal], () => listener(signal))),
    );
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
      filters: [{ name: 'Wirecatch settings', extensions: ['json'] }],
    });
  }
  async pickOpenFile() {
    const picked = await open({
      multiple: false,
      directory: false,
      filters: [{ name: 'Wirecatch settings', extensions: ['json'] }],
    });
    return typeof picked === 'string' ? picked : null;
  }
}
