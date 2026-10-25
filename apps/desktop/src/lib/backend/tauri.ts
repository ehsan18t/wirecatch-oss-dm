// Talks to the Rust side through the commands of the generated contract (`CommandMap`, from `crates/protocol/src/desktop.rs`) and the single `UI_EVENT` stream. Each command takes its arguments as one `args` object keyed by the Rust names.

import { PRODUCT_NAME } from '@wirecatch/product';
import { getName } from '@tauri-apps/api/app';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { open, save } from '@tauri-apps/plugin-dialog';
import {
  COMMAND_OUTPUTS,
  FINISHED_EVENT,
  isErrorCode,
  isFinishedList,
  isReveal,
  isSystemAppearance,
  isUiEvent,
  isWindowFrame,
  LINKS_EVENT,
  RESYNC_EVENT,
  REVEAL_EVENT,
  SYSTEM_APPEARANCE_EVENT,
  UI_EVENT,
  WINDOW_FRAME_EVENT,
  type AddonId,
  type AddRequest,
  type AppSettingsPatch,
  type ButtonArea,
  type CaptureConfirmation,
  type CommandMap,
  type DownloadId,
  type FinishedList,
  type MediaSelection,
  type ProxyChange,
  type ProxyTest,
  type QuestionId,
  type QueueSettings,
  type QueueId,
  type Reveal,
  type SystemAppearance,
  type UiEvent,
  type WindowAction,
  type WindowFrame,
  type LinkTrial,
} from '@wirecatch/protocol';
import { keysOf } from '../keys';
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

/** Tauri's `invoke`; tests pass their own. Its answer is checked before use, so it is `unknown` here. */
type Invoke = (command: string, args?: Record<string, unknown>) => Promise<unknown>;

/** A command's name in the contract. */
type Command = keyof CommandMap;
/** What a command takes: nothing for one whose `args` is `null`, else its `args` object. */
type ArgsOf<C extends Command> = CommandMap[C]['args'] extends null ? [] : [CommandMap[C]['args']];
/** What a command answers with. */
type OutputOf<C extends Command> = CommandMap[C]['output'];
/** Tauri's `listen`; tests pass their own. Nothing checks what an event carries before it arrives, so its payload is `unknown` here, and a type argument cannot claim otherwise. */
type Listen = (
  event: string,
  handler: (event: { payload: unknown }) => void,
) => Promise<Unsubscribe>;

/** How often, at most, a kind of event the window cannot read is reported and reloaded for: every time would reload in a loop, and only once would leave the window frozen while the mismatch lasts. */
export const UNREADABLE_RELOAD_MS = 60_000;

/** The `type` an event that failed its check says it is, or null when it says none. */
function claimedKind(payload: unknown): string | null {
  return typeof payload === 'object' &&
    payload !== null &&
    'type' in payload &&
    typeof payload.type === 'string'
    ? payload.type
    : null;
}

/** Why a command failed when the shell's answer does not have the contract's shape: a bug, never the person's doing. */
export function unreadableAnswer(command: string): string {
  return `${PRODUCT_NAME} sent an answer to ${command} that this window cannot read. Restart ${PRODUCT_NAME}, and please report it if it keeps happening.`;
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
  /** When each kind of event found unreadable was last reported and reloaded for. */
  readonly #unreadableAt = new Map<string | null, number>();

  constructor(invoker: Invoke = invoke, listener: Listen = listen) {
    this.#invoke = invoker;
    this.#listen = listener;
  }

  /** Calls `command` with its contract arguments, and checks the answer has the contract's shape. */
  async #call<C extends Command>(command: C, ...args: ArgsOf<C>): Promise<OutputOf<C>> {
    let answer: unknown;
    try {
      answer = await this.#invoke(command, args.length === 0 ? undefined : { args: args[0] });
    } catch (error) {
      throw toBackendError(error);
    }
    const fits: (value: unknown) => value is OutputOf<C> = COMMAND_OUTPUTS[command];
    if (!fits(answer)) throw new BackendError(unreadableAnswer(command));
    return answer;
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
  addVideoPage(request: AddRequest) {
    return this.#call('add_video_page', { request });
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
  createQueue(settings: QueueSettings) {
    return this.#call('create_queue', { settings });
  }
  updateQueue(id: QueueId, settings: QueueSettings) {
    return this.#exec('update_queue', { id, settings });
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
  openFile(id: DownloadId) {
    return this.#exec('open_file', { id });
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
  suffixList() {
    return this.#call('suffix_list');
  }
  checkSuffixList() {
    return this.#call('check_suffix_list');
  }
  setProxy(change: ProxyChange) {
    return this.#exec('set_proxy', { change });
  }
  tryLink(trial: LinkTrial) {
    return this.#call('try_link', { trial });
  }

  testProxy(test: ProxyTest) {
    return this.#call('test_proxy', { test });
  }
  systemProxy() {
    return this.#call('system_proxy');
  }
  showProxySettings() {
    return this.#exec('show_proxy_settings');
  }

  /** Passes on every event with the contract's shape. One without it is a bug on one side: it is dropped, reported to the shell's log, and `reload` is called to load everything again from the commands, whose answers are checked; for a kind that keeps failing, at most once every `UNREADABLE_RELOAD_MS`. */
  async subscribe(listener: (event: UiEvent) => void, reload: () => void): Promise<Unsubscribe> {
    return this.#listen(UI_EVENT, ({ payload }) => {
      if (isUiEvent(payload)) listener(payload);
      else if (this.#reportUnreadable(claimedKind(payload))) reload();
    });
  }

  /** Reports an unreadable event of `kind` unless one was reported within `UNREADABLE_RELOAD_MS`; whether it did. */
  #reportUnreadable(kind: string | null): boolean {
    const now = Date.now();
    const last = this.#unreadableAt.get(kind);
    if (last !== undefined && now - last < UNREADABLE_RELOAD_MS) return false;
    this.#unreadableAt.set(kind, now);
    // The report is only for the log: a failure to send it changes nothing for the person.
    this.#exec('report_unreadable_event', { kind }).catch(() => undefined);
    return true;
  }

  async onShellSignal(listener: (signal: ShellSignal) => void): Promise<Unsubscribe> {
    const signals = keysOf(SIGNAL_EVENTS);
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

  /** Listens to a shell event whose payload is checked against the contract; one that does not fit is dropped. */
  #listenChecked<T>(
    event: string,
    fits: (value: unknown) => value is T,
    listener: (payload: T) => void,
  ): Promise<Unsubscribe> {
    return this.#listen(event, (e) => {
      if (fits(e.payload)) listener(e.payload);
    });
  }

  takeReveal() {
    return this.#call('take_reveal');
  }
  onReveal(listener: (reveal: Reveal) => void) {
    return this.#listenChecked(REVEAL_EVENT, isReveal, listener);
  }
  finishedList() {
    return this.#call('finished_list');
  }
  onFinished(listener: (list: FinishedList) => void) {
    return this.#listenChecked(FINISHED_EVENT, isFinishedList, listener);
  }
  startupNotices() {
    return this.#call('startup_notices');
  }
  showSetAsideList() {
    return this.#exec('show_set_aside_list');
  }
  systemAppearance() {
    return this.#call('system_appearance');
  }
  onSystemAppearance(listener: (appearance: SystemAppearance) => void) {
    return this.#listenChecked(SYSTEM_APPEARANCE_EVENT, isSystemAppearance, listener);
  }
  controlWindow(action: WindowAction) {
    return this.#exec('control_window', { action });
  }
  windowFrame() {
    return this.#call('window_frame');
  }
  onWindowFrame(listener: (frame: WindowFrame) => void) {
    return this.#listenChecked(WINDOW_FRAME_EVENT, isWindowFrame, listener);
  }
  setMaximizeButton(area: ButtonArea | null) {
    return this.#exec('set_maximize_button', { area });
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
