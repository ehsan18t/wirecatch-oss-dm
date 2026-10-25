// An in-memory stand-in for the core, so the interface can be built, reviewed and tested in a
// normal browser. It simulates queues, limits, schedules, varied speeds, failures with fixes,
// and browser captures, and pushes the same `UiEvent`s the real core will.

import { PRODUCT_NAME, PRODUCT_SLUG } from '@wirecatch/product';
import type {
  AddRequest,
  AppSettingsPatch,
  CaptureConfirmation,
  DownloadId,
  MediaSelection,
  ProxyChange,
  ProxyTest,
  QuestionId,
  QueueId,
  QueueSettings,
  SettingsProfile,
  StartupNotice,
  UiEvent,
  LinkTrial,
} from '@wirecatch/protocol';
import { folderOf } from '../../format';
import { WindowRegistry } from '../../sample-windows';
import type { Backend, SampleData, Unsubscribe } from '../types';
import * as addons from './addons';
import * as captures from './captures';
import { MockFinished } from './finished';
import * as suffixList from './suffix-list';
import * as data from './data';
import * as downloads from './downloads';
import * as media from './media';
import { populate } from './populate';
import * as queues from './queues';
import { mulberry32 } from './rng';
import { MockShell } from './shell';
import { sampleCommands } from './sample-commands';
import { SEEDS } from './seeds';
import * as settings from './settings';
import { DEFAULT_TICK_MS, step } from './simulator';
import type { MockOptions } from './options';
import { MockState } from './state';

/** What a call to the mock core came to: its value, or the error it failed with. */
type Outcome<T> = { ok: true; value: T } | { ok: false; error: unknown };

export class MockBackend extends MockShell implements Backend {
  readonly kind = 'mock';

  appName() {
    return Promise.resolve(PRODUCT_NAME);
  }
  /**
   * The download windows (D23). A browser tab cannot open real windows that share this sample
   * data, so the page shows them inside itself (`MockWindows.svelte`), with the desktop's rules.
   */
  readonly windows = new WindowRegistry();
  readonly sampleData: SampleData;

  /** Set by tests and sample data: what the next update check finds. */
  nextUpdateCheck: addons.UpdateCheck = 'up-to-date';
  /** Set by tests: what the next check of the Public Suffix List finds. */
  nextSuffixCheck: suffixList.SuffixCheck = 'up-to-date';
  /** Files opened, for tests: the mock opens nothing for real. */
  readonly opened: DownloadId[] = [];
  /**
   * How the sample asks before it opens a program. The shell asks in a dialog of its own, which no
   * page can answer; the browser's confirm is the sample's equivalent. Tests replace it.
   */
  answerProgramWarning: (warning: string) => boolean = (warning) => window.confirm(warning);

  readonly #state: MockState;
  /** The download complete window's list (D52). */
  readonly #finished: MockFinished;
  readonly #latencyMs: number;
  readonly #startupNotices: StartupNotice[];
  #timer: ReturnType<typeof setInterval> | undefined;
  /** Everything still to happen later (answers waiting out `latencyMs`, sample events), so `dispose` stops it. */
  readonly #later = new Set<ReturnType<typeof setTimeout>>();
  #lastTick: number;
  #captureIndex = 0;
  #mediaIndex = 0;
  #folderPick = 0;

  constructor(options: MockOptions = {}) {
    super();
    const now = options.now ?? (() => Date.now());
    this.#state = new MockState(this.windows, mulberry32(options.seed ?? data.DEFAULT_SEED), now);
    this.#latencyMs = options.latencyMs ?? 0;
    this.#startupNotices = data.startupNoticesFor(options);
    this.#lastTick = now();
    if (!options.empty) populate(this.#state, options.count ?? SEEDS.length, options.active ?? 0);
    this.#finished = new MockFinished(this.#state);
    this.sampleData = {
      windows: this.windows,
      commands: sampleCommands(this, this.#finished),
      now: () => this.#state.now(),
      finished: this.#finished,
    };
    // Work out what runs and why the rest waits before anyone asks.
    this.tick(0);
    this.#startTimers(options);
  }

  /** The simulation's clock, and the sample capture a few seconds after start, as the options ask. */
  #startTimers({
    autoTick = true,
    tickMs = DEFAULT_TICK_MS,
    autoCapture = false,
  }: MockOptions): void {
    if (autoTick) this.#timer = setInterval(() => this.tick(), tickMs);
    if (autoCapture) this.#after(data.AUTO_CAPTURE_MS, () => this.simulateCapture());
  }

  /** Stops the timers, including answers still on their way back; those never arrive. */
  dispose(): void {
    if (this.#timer) clearInterval(this.#timer);
    for (const timer of this.#later) clearTimeout(timer);
    this.#later.clear();
  }

  /** Runs `work` in `ms`, unless `dispose` comes first. */
  #after(ms: number, work: () => void): void {
    const timer = setTimeout(() => {
      this.#later.delete(timer);
      work();
    }, ms);
    this.#later.add(timer);
  }

  /**
   * One call to the mock core. The work happens at once, as if the request reached the core
   * immediately, so its events go out first. Without latency the answer (or the error) settles
   * the returned promise straight away; with `latencyMs` it comes back that much later, as from
   * the real core over IPC.
   */
  protected answer<T>(work: () => T): Promise<T> {
    return this.#call(work);
  }

  async #call<T>(work: () => T): Promise<T> {
    let outcome: Outcome<T>;
    try {
      outcome = { ok: true, value: work() };
    } catch (error) {
      outcome = { ok: false, error };
    }
    if (this.#latencyMs > 0) await this.#delay(this.#latencyMs);
    if (!outcome.ok) throw outcome.error;
    return outcome.value;
  }

  /** Waits `ms`, unless `dispose` stops the wait first. */
  #delay(ms: number): Promise<void> {
    return new Promise((resolve) => this.#after(ms, resolve));
  }

  /** Advances the simulation. Called on a timer, or directly by tests. */
  tick(elapsedMs?: number): void {
    const now = this.#state.now();
    const dt = elapsedMs ?? Math.max(1, now - this.#lastTick);
    this.#lastTick = now;
    const resumed = step(this.#state, dt);
    if (resumed.length > 0) {
      // What asked for the add-on continues on its own, under the same id.
      queueMicrotask(() => {
        for (const page of resumed) this.simulateMediaChoice(page.url, page.id);
      });
    }
  }

  listDownloads() {
    return this.#call(() => downloads.listDownloads(this.#state));
  }

  addDownload(request: AddRequest) {
    return this.#call(() => downloads.addDownload(this.#state, request));
  }

  /** Like the core: the page is read (a moment here), then asks or downloads with the saved choices. */
  addVideoPage(request: AddRequest) {
    return this.#call(() => this.#readSlowly(request.url));
  }

  pause(id: DownloadId) {
    return this.#call(() => downloads.pause(this.#state, id));
  }

  resume(id: DownloadId) {
    return this.#call(() => downloads.resume(this.#state, id));
  }

  cancel(id: DownloadId) {
    return this.#call(() => downloads.cancel(this.#state, id));
  }

  remove(ids: DownloadId[], _deleteFile: boolean) {
    return this.#call(() => downloads.remove(this.#state, ids));
  }

  retry(id: DownloadId) {
    return this.#call(() => downloads.retry(this.#state, id));
  }

  refreshLink(id: DownloadId, url: string) {
    return this.#call(() => downloads.refreshLink(this.#state, id, url));
  }

  keepOriginal(id: DownloadId) {
    return this.#call(() => downloads.keepOriginal(this.#state, id));
  }

  moveToQueue(ids: DownloadId[], queue: QueueId) {
    return this.#call(() => downloads.moveToQueue(this.#state, ids, queue));
  }

  reorder(queue: QueueId, idsInOrder: DownloadId[]) {
    return this.#call(() => downloads.reorder(this.#state, queue, idsInOrder));
  }

  pauseAll() {
    return this.#call(() => downloads.pauseAll(this.#state));
  }

  resumeAll() {
    return this.#call(() => downloads.resumeAll(this.#state));
  }

  getDownload(id: DownloadId) {
    return this.#call(() => ({ ...this.#state.row(id) }));
  }

  listQueues() {
    return this.#call(() => queues.listQueues(this.#state));
  }

  createQueue(settings: QueueSettings) {
    return this.#call(() => queues.createQueue(this.#state, settings));
  }

  updateQueue(id: QueueId, settings: QueueSettings) {
    return this.#call(() => queues.updateQueue(this.#state, id, settings));
  }

  setQueuePaused(id: QueueId, paused: boolean) {
    return this.#call(() => {
      queues.setQueuePaused(this.#state, id, paused);
      this.tick(0);
    });
  }

  startQueueNow(id: QueueId) {
    return this.#call(() => {
      queues.startQueueNow(this.#state, id);
      this.tick(0);
    });
  }

  deleteQueue(id: QueueId) {
    return this.#call(() => queues.deleteQueue(this.#state, id));
  }

  getSettings() {
    return this.#call(() => structuredClone(this.#state.settings));
  }

  changeSettings(patch: AppSettingsPatch) {
    return this.#call(() => settings.changeSettings(this.#state, patch));
  }

  exportSettings(path: string) {
    return this.#call(() => settings.exportSettings(this.#state, path));
  }

  /** Puts a settings file where the file picker finds it, as if made elsewhere. */
  putSettingsFile(path: string, profile: SettingsProfile) {
    this.#state.files.set(path, structuredClone(profile));
  }

  previewImport(path: string) {
    return this.#call(() => settings.previewImport(this.#state, path));
  }

  importSettings(path: string) {
    return this.#call(() => settings.importSettings(this.#state, path));
  }

  pendingConfirmations() {
    return this.#call(() => this.#state.pending.map((p) => ({ ...p })));
  }

  confirmCapture(id: QuestionId, confirmation: CaptureConfirmation) {
    return this.#call(() => captures.confirmCapture(this.#state, id, confirmation));
  }

  rejectCapture(id: QuestionId) {
    return this.#call(() => captures.rejectCapture(this.#state, id));
  }

  getQuestion(id: QuestionId) {
    return this.#call(() => captures.getQuestion(this.#state, id));
  }

  earlierDownload(id: QuestionId) {
    return this.#call(() => captures.earlierDownload(this.#state, id));
  }

  showQuestionWindows() {
    return this.#call(() => this.windows.showAll(this.#state.openQuestions()));
  }

  mainWindowReady() {
    // A browser tab is already on screen.
    return this.#call(() => undefined);
  }

  showProgressWindow(id: DownloadId) {
    return this.#call(() => {
      this.#state.row(id);
      this.windows.showDownload(id);
    });
  }

  /** Sample data only: the oldest open question is answered somewhere else, so the core closes it. */
  simulateAnsweredElsewhere(): void {
    captures.simulateAnsweredElsewhere(this.#state);
  }

  /** Sample data only: pretends the browser extension handed over a download. */
  simulateCapture(): void {
    captures.simulateCapture(this.#state, this.#captureIndex++);
  }

  /** Sample data only: a capture that asks first, whatever the setting says. */
  simulateConfirmation(): void {
    captures.simulateConfirmation(this.#state, this.#captureIndex++);
  }

  /** Sample data only: the browser found media with several qualities. */
  simulateMediaChoice(page?: string, id?: QuestionId): void {
    media.simulateMediaChoice(this.#state, this.#mediaIndex++, page, id);
  }

  /** Sample data only: a page that takes a while to read. */
  simulateSlowReading(): void {
    this.#readSlowly(media.SLOW_SAMPLE_PAGE);
  }

  /** A page whose qualities show after three seconds, unless it is cancelled first. */
  #readSlowly(page: string): QuestionId {
    const id = media.startReading(this.#state, page);
    const stillReading = () => this.#state.reading.some((r) => r.id === id);
    this.#after(data.SLOW_READING_MS, () => stillReading() && this.simulateMediaChoice(page, id));
    return id;
  }

  /** Sample data only: a page that needs the media add-on to download. */
  simulateAddonRequired(): void {
    if (this.#state.mediaReady()) this.simulateMediaChoice(media.ADDON_SAMPLE_PAGE);
    else media.askForAddon(this.#state, media.ADDON_SAMPLE_PAGE);
  }

  pendingMediaChoices() {
    return this.#call(() => structuredClone(this.#state.mediaChoices));
  }

  pendingAddonRequests() {
    return this.#call(() => media.pendingAddonRequests(this.#state));
  }

  chooseMedia(id: QuestionId, selection: MediaSelection) {
    return this.#call(() => media.chooseMedia(this.#state, id, selection));
  }

  dismissMediaChoice(id: QuestionId) {
    return this.#call(() => media.dismissMediaChoice(this.#state, id));
  }

  listAddons() {
    return this.#call(() => structuredClone(this.#state.addons));
  }

  installAddon(id: string) {
    return this.#call(() => addons.installAddon(this.#state, id));
  }

  updateAddon(id: string) {
    return this.#call(() => addons.installAddon(this.#state, id));
  }

  checkAddonUpdates() {
    return this.#call(() => addons.checkAddonUpdates(this.#state, this.nextUpdateCheck));
  }

  suffixList() {
    return this.#call(() => ({ ...this.#state.suffixList }));
  }

  checkSuffixList() {
    return this.#call(() => suffixList.checkSuffixList(this.#state, this.nextSuffixCheck));
  }

  setProxy(change: ProxyChange) {
    return this.#call(() => settings.setProxy(this.#state, change));
  }

  setQueueOrder(order: QueueId[]) {
    return this.#call(() => queues.setQueueOrder(this.#state, order));
  }

  tryLink(trial: LinkTrial) {
    return this.#call(() => queues.tryLink(this.#state, trial));
  }

  testProxy(test: ProxyTest) {
    return this.#call(() => settings.testProxy(this.#state, test));
  }

  systemProxy() {
    return this.#call(() => settings.systemProxy());
  }

  showProxySettings() {
    return this.#call(() => undefined);
  }

  /** Set by tests: the core's weekly check of the Public Suffix List runs, finding `finds`. */
  simulateWeeklySuffixCheck(finds: suffixList.SuffixCheck): void {
    suffixList.weeklyCheck(this.#state, finds);
  }

  cancelAddonOperation(id: string) {
    return this.#call(() => addons.cancelAddonOperation(this.#state, id));
  }

  removeAddon(id: string) {
    return this.#call(() => addons.removeAddon(this.#state, id));
  }

  takeLaunchLinks() {
    return this.#call((): string[] => []);
  }

  startupNotices() {
    return this.#call((): StartupNotice[] => [...this.#startupNotices]);
  }

  /** Its events are always readable, so it never asks for a reload. */
  subscribe(listener: (event: UiEvent) => void, _reload: () => void) {
    return this.#call((): Unsubscribe => {
      const listeners = this.#state.listeners;
      listeners.add(listener);
      return () => listeners.delete(listener);
    });
  }

  openFile(id: DownloadId) {
    return this.#call(() => {
      if (downloads.mayOpen(this.#state, id, this.answerProgramWarning)) this.opened.push(id);
    });
  }

  openFolder(id: DownloadId) {
    return this.#call(() => {
      this.#state.row(id);
    });
  }

  setDownloadFolder(id: DownloadId, folder: string) {
    return this.#call(() => downloads.setDownloadFolder(this.#state, id, folder));
  }

  pickFolder(current?: string) {
    return this.#call(() => {
      const next = () =>
        data.PICKED_FOLDERS[this.#folderPick++ % data.PICKED_FOLDERS.length] ?? data.DOWNLOADS;
      const first = next();
      return first === current ? next() : first;
    });
  }

  pickSaveFile(suggestedName: string) {
    return this.#call(() => `${data.HOME}\\Documents\\${suggestedName}`);
  }

  pickOpenFile() {
    return this.#call(
      () =>
        [...this.#state.files.keys()].pop() ??
        `${data.HOME}\\Documents\\${PRODUCT_SLUG}-settings.json`,
    );
  }

  /** Test helper: the folder a row will land in. */
  folderOf(id: DownloadId): string {
    return folderOf(this.#state.row(id).path);
  }
}
