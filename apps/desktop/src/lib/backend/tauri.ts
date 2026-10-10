// Talks to the Rust side through Tauri commands and the single `wirecatch://event` stream.
// Argument keys are camelCase here; Tauri maps them to the snake_case Rust parameter names.

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { open, save } from '@tauri-apps/plugin-dialog';
import type {
  AddRequest,
  AppSettings,
  CaptureConfirmation,
  DownloadId,
  DownloadSummary,
  PendingCapture,
  Queue,
  QueueId,
  UiEvent,
} from '@wirecatch/protocol';
import { BackendError, COMMANDS, EVENT_NAME, type Backend, type Unsubscribe } from './types';

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
  importSettings(path: string) {
    return this.#exec(COMMANDS.importSettings, { path });
  }
  pendingConfirmations() {
    return this.#call<PendingCapture[]>(COMMANDS.pendingConfirmations);
  }
  confirmCapture(id: number, confirmation: CaptureConfirmation) {
    return this.#exec(COMMANDS.confirmCapture, { id, confirmation });
  }
  rejectCapture(id: number) {
    return this.#exec(COMMANDS.rejectCapture, { id });
  }
  openFile(id: DownloadId) {
    return this.#exec(COMMANDS.openFile, { id });
  }
  openFolder(id: DownloadId) {
    return this.#exec(COMMANDS.openFolder, { id });
  }
  setDownloadFolder(id: DownloadId, folder: string) {
    return this.#exec(COMMANDS.setDownloadFolder, { id, folder });
  }

  async subscribe(listener: (event: UiEvent) => void): Promise<Unsubscribe> {
    return listen<UiEvent>(EVENT_NAME, (e) => listener(e.payload));
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
