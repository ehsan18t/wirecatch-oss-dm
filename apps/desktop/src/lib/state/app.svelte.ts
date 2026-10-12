// Everything the main window knows and every action it can take. Components read from here and
// call these methods; nothing else talks to the backend. Each concern has its own class (the
// selection, the queues, the notes, the questions, the add-ons, settings files); this one owns
// the list, the screens and dialogs, the actions on downloads, and the core's events.

import type {
  AddRequest,
  AppSettings,
  DownloadId,
  FixAction,
  QueueId,
  UiEvent,
} from '@wirecatch/protocol';
import { getContext, setContext, untrack } from 'svelte';
import type { Backend } from '../backend';
import { extractUrls } from '../format';
import type { SettingId } from '../settings-schema';
import { Addons } from './addons.svelte';
import { Clock } from './clock.svelte';
import { DownloadStore } from './downloads.svelte';
import { mainWindowEvents } from './app-events';
import { dispatch, HeldEvents } from './events';
import { Hints } from './hints.svelte';
import { nudged, placed } from './order';
import { Questions } from './questions.svelte';
import { Queues } from './queues.svelte';
import { errorMessage, Runner } from './runner';
import { Selection } from './selection.svelte';
import { changeSettings, SettingsWriter } from './settings-changes.svelte';
import { SettingsTransfer } from './settings-transfer.svelte';
import { Toasts } from './toasts.svelte';
import { countRows, isUnderWay, visibleIds, type SortKey, type View } from './view';

export type { Toast } from './toasts.svelte';

export type Screen = 'downloads' | 'settings';

export type Dialog =
  | { kind: 'add'; urls: string[]; queue?: QueueId }
  | { kind: 'remove'; ids: DownloadId[] }
  | { kind: 'cancel'; ids: DownloadId[] }
  | { kind: 'queue'; id: QueueId | null }
  | { kind: 'refresh'; id: DownloadId }
  | { kind: 'export' }
  | { kind: 'import' }
  | { kind: 'shortcuts' }
  | { kind: 'open-program'; id: DownloadId }
  | { kind: 'remove-addon'; id: string };

const CONTEXT = Symbol('wirecatch-app');

export function setApp(app: AppState): void {
  setContext(CONTEXT, app);
}

export function getApp(): AppState {
  const app = getContext<AppState | undefined>(CONTEXT);
  if (!app) throw new Error('AppState is missing; wrap the component in the app shell.');
  return app;
}

export class AppState {
  #backend: Backend | null = null;
  readonly #loadBackend: () => Promise<Backend>;
  readonly downloads = new DownloadStore();
  readonly hints = new Hints();
  readonly toasts = new Toasts();
  /** The time schedule labels follow. */
  readonly clock = new Clock();
  readonly #runner = new Runner((text) => this.toasts.fail(text));
  readonly selection = new Selection(
    () => this.visible,
    (id) => this.downloads.get(id),
  );
  readonly questions = new Questions();
  readonly addons = new Addons(() => this.backend, this.#runner, this.toasts);
  readonly queues = new Queues({
    backend: () => this.backend,
    runner: this.#runner,
    toasts: this.toasts,
    now: () => this.clock.now,
    counts: () => this.counts,
  });
  readonly transfer = new SettingsTransfer({
    backend: () => this.backend,
    runner: this.#runner,
    imported: (settings, queues) => {
      if (settings) this.settings = settings;
      if (queues) this.queues.list = queues;
    },
  });
  /** Settings changes go to the app one after another, in the order they were made. */
  readonly #settingsWriter = new SettingsWriter((patch) =>
    this.#runner.succeeds(() => this.backend.changeSettings(patch)),
  );

  settings = $state<AppSettings | null>(null);
  ready = $state(false);
  loadError = $state<string | null>(null);

  screen = $state<Screen>('downloads');
  /** The setting to scroll to and focus when the settings screen opens. */
  settingsFocus = $state<SettingId | null>(null);
  view = $state<View>({ kind: 'all' });
  search = $state('');
  sort = $state<SortKey>('queue');

  /** The window is wide enough for the detail panel to have its own column. */
  wide = $state(true);
  /** In a wide window, whether the detail panel column is shown. */
  detailsOpen = $state(true);

  dialog = $state<Dialog | null>(null);
  paletteOpen = $state(false);
  /** Saving the download list failed; shown in a banner until dealt with. */
  storageProblem = $state<string | null>(null);
  /** True when the interface runs on sample data instead of the real engine. */
  sampleData = $state(false);

  #unsubscribe: (() => void) | null = null;
  /** Bumped by every init and dispose, so a superseded init cleans up after itself. */
  #generation = 0;

  /** Takes a backend, or a function that loads one (so the mock can stay out of production). */
  constructor(backend: Backend | (() => Promise<Backend>)) {
    if (typeof backend === 'function') {
      this.#loadBackend = backend;
    } else {
      this.#backend = backend;
      this.#loadBackend = () => Promise.resolve(backend);
    }
  }

  /** The backend. Available once `init` has started; components only use it after loading. */
  get backend(): Backend {
    if (!this.#backend) throw new Error('The backend is not loaded yet.');
    return this.#backend;
  }

  // ---- Derived views ----

  readonly #get = (id: DownloadId) => this.downloads.get(id);

  /** Ids the list shows. Rebuilt only when membership, view, search or sort change. */
  visible = $derived.by(() => {
    const order = this.downloads.order;
    const options = { view: this.view, search: this.search, sort: this.sort };
    return untrack(() => visibleIds(order, this.#get, options));
  });

  counts = $derived.by(() => {
    const order = this.downloads.order;
    return untrack(() => countRows(order, this.#get));
  });

  // ---- Startup and events ----

  /**
   * Loads everything and starts listening. Safe to call again (the Try again button does): the
   * previous listener and clock are torn down first, and an init that is overtaken by a newer
   * init or by `dispose` drops what it set up.
   */
  async init(): Promise<void> {
    this.#teardown();
    const generation = ++this.#generation;
    const current = () => generation === this.#generation;
    const events = new HeldEvents((e) => this.handle(e));
    const unsubscribe = await this.#connect(current, events.receive);
    if (!unsubscribe) return;
    if (!current()) {
      unsubscribe();
      return;
    }
    this.#unsubscribe = unsubscribe;
    await this.#load(current);
    if (!current()) {
      events.drop();
      return;
    }
    events.release();
    this.ready = true;
    this.clock.start();
    // Neither is needed for the list, so neither holds it up.
    void this.addons.load();
    void this.#takeLaunchLinks();
    void this.questions.restore(this.backend);
  }

  /** Loads the backend and listens to it; `null` when that failed, which is then shown. */
  async #connect(
    current: () => boolean,
    onEvent: (e: UiEvent) => void,
  ): Promise<(() => void) | null> {
    try {
      const backend = this.#backend ?? (await this.#loadBackend());
      this.#backend = backend;
      this.sampleData = backend.kind === 'mock';
      this.clock.follow(backend.sampleData?.now ?? (() => Date.now()));
      const stopEvents = await backend.subscribe(onEvent);
      const stopSignals = await backend.onShellSignal((signal) => {
        if (!current()) return;
        // Events were missed: load everything again rather than show a stale list.
        if (signal === 'resync') void this.init();
        else void this.#takeLaunchLinks();
      });
      return () => {
        stopEvents();
        stopSignals();
      };
    } catch (err) {
      if (current()) {
        this.loadError = errorMessage(err);
        this.ready = true;
      }
      return null;
    }
  }

  /** Loads the list, the queues, the settings and the waiting browser files. */
  async #load(current: () => boolean): Promise<void> {
    try {
      const [downloads, queues, settings, pending] = await Promise.all([
        this.backend.listDownloads(),
        this.backend.listQueues(),
        this.backend.getSettings(),
        this.backend.pendingConfirmations(),
      ]);
      if (!current()) return;
      this.downloads.load(downloads);
      this.queues.list = queues;
      this.settings = settings;
      this.questions.pending = pending;
      this.loadError = null;
    } catch (err) {
      if (current()) this.loadError = errorMessage(err);
    }
  }

  /** Links from the command line (`wirecatch-app https://...`) open the Add dialog. */
  async #takeLaunchLinks(): Promise<void> {
    let links: string[];
    try {
      links = await this.backend.takeLaunchLinks();
    } catch {
      return;
    }
    if (links.length === 0) return;
    const open = this.dialog;
    const urls = open?.kind === 'add' ? [...open.urls, ...links] : links;
    this.openDialog({ kind: 'add', urls });
  }

  #teardown(): void {
    this.#unsubscribe?.();
    this.#unsubscribe = null;
    this.clock.stop();
  }

  dispose(): void {
    this.#generation += 1;
    this.#teardown();
  }

  readonly #handlers = mainWindowEvents(this);

  handle(event: UiEvent): void {
    dispatch(this.#handlers, event);
  }

  // ---- Navigation ----

  show(view: View): void {
    this.view = view;
    this.screen = 'downloads';
    this.selection.clear();
  }

  openSettings(focus: SettingId | null = null): void {
    this.screen = 'settings';
    this.settingsFocus = focus;
    this.paletteOpen = false;
  }

  openDialog(dialog: Dialog): void {
    this.paletteOpen = false;
    this.dialog = dialog;
  }

  closeDialog(): void {
    this.dialog = null;
    // A settings file previewed but not imported is left as it was.
    this.transfer.cancelImport();
  }

  /** Pasted text: open the Add dialog with every link in it. */
  pasteText(text: string): boolean {
    const urls = extractUrls(text);
    if (urls.length === 0) {
      this.toasts.fail(
        'There is no link in what you pasted. Copy the full address, starting with https://.',
      );
      return false;
    }
    this.openDialog({ kind: 'add', urls });
    return true;
  }

  /** Brings every waiting question's window to the front, opening closed ones again. */
  async showQuestionWindows(): Promise<void> {
    this.paletteOpen = false;
    await this.#runner.run(() => this.backend.showQuestionWindows());
  }

  /** Opens (or brings forward) a download's own progress window. */
  async showProgressWindow(id: DownloadId): Promise<void> {
    this.paletteOpen = false;
    await this.#runner.run(() => this.backend.showProgressWindow(id));
  }

  // ---- Download actions ----

  async add(requests: AddRequest[]): Promise<DownloadId[]> {
    const ids: DownloadId[] = [];
    for (const r of requests) {
      const id = await this.#runner.run(() => this.backend.addDownload(r));
      if (id !== undefined) ids.push(id);
    }
    if (ids.length > 0) {
      this.toasts.notify(ids.length === 1 ? 'Download added.' : `${ids.length} downloads added.`);
    }
    return ids;
  }

  async pause(ids: DownloadId[] = this.selection.list): Promise<void> {
    await this.#runner.bulk(ids, 'pause', (id) => this.backend.pause(id));
  }

  async resume(ids: DownloadId[] = this.selection.list): Promise<void> {
    await this.#runner.bulk(ids, 'resume', (id) => this.backend.resume(id));
  }

  /** Space bar: pause what runs, resume what does not. */
  async togglePause(ids: DownloadId[] = this.selection.list): Promise<void> {
    const rows = ids.flatMap((id) => this.downloads.get(id) ?? []);
    if (rows.some((r) => isUnderWay(r.status))) {
      await this.pause(ids);
      return;
    }
    // A cancelled download has no partial file left; it starts over.
    const cancelled = rows.filter((r) => r.status === 'cancelled').map((r) => r.id);
    await this.retry(cancelled);
    await this.resume(ids.filter((id) => !cancelled.includes(id)));
  }

  async retry(ids: DownloadId[] = this.selection.list): Promise<void> {
    await this.#runner.bulk(ids, 'restart', (id) => this.backend.retry(id));
  }

  /** Tries every download that needs attention again. */
  async retryAllFailed(): Promise<void> {
    const failed = this.downloads.order.filter((id) => this.downloads.get(id)?.status === 'failed');
    if (failed.length === 0) {
      this.toasts.notify('Nothing needs attention right now.');
      return;
    }
    const ok = await this.#runner.bulk(failed, 'restart', (id) => this.backend.retry(id));
    if (ok > 0)
      this.toasts.notify(ok === 1 ? 'Trying 1 download again.' : `Trying ${ok} downloads again.`);
  }

  async cancel(ids: DownloadId[] = this.selection.list): Promise<void> {
    await this.#runner.bulk(ids, 'cancel', (id) => this.backend.cancel(id));
  }

  async remove(ids: DownloadId[], deleteFile: boolean): Promise<void> {
    await this.#runner.run(() => this.backend.remove(ids, deleteFile));
  }

  refreshLink(id: DownloadId, url: string): Promise<boolean> {
    return this.#runner.succeeds(() => this.backend.refreshLink(id, url));
  }

  /** Opens a finished file; a program asks first whether the person trusts where it came from. */
  async openFile(id: DownloadId): Promise<void> {
    const program = await this.#runner.run(() => this.backend.isProgram(id));
    if (program === undefined) return;
    if (program) {
      this.openDialog({ kind: 'open-program', id });
      return;
    }
    await this.#runner.run(() => this.backend.openFile(id));
  }

  /** Opens a program the person confirmed they trust. */
  async openProgram(id: DownloadId): Promise<void> {
    this.closeDialog();
    await this.#runner.run(() => this.backend.openFile(id, true));
  }

  async openFolder(id: DownloadId): Promise<void> {
    await this.#runner.run(() => this.backend.openFolder(id));
  }

  /** Lets the person pick a new folder for one download. */
  async chooseFolderFor(id: DownloadId): Promise<boolean> {
    const row = this.downloads.get(id);
    const picked = await this.#runner.run(() => this.backend.pickFolder(row?.path));
    if (!picked) return false;
    const ok = await this.#runner.succeeds(() => this.backend.setDownloadFolder(id, picked));
    if (ok) this.toasts.notify(`Saving to ${picked}`);
    return ok;
  }

  /** Runs the fix an error offers. */
  async fix(id: DownloadId, action: FixAction): Promise<void> {
    switch (action) {
      case 'retry':
        await this.retry([id]);
        break;
      case 'refresh_link':
        this.openDialog({ kind: 'refresh', id });
        break;
      case 'choose_folder':
        if (await this.chooseFolderFor(id)) await this.retry([id]);
        break;
      case 'free_disk_space':
        await this.openFolder(id);
        break;
      case 'keep_original':
        await this.#runner.run(() => this.backend.keepOriginal(id));
        break;
    }
  }

  async moveToQueue(ids: DownloadId[], queue: QueueId): Promise<void> {
    if (ids.length === 0) return;
    const ok = await this.#runner.succeeds(() => this.backend.moveToQueue(ids, queue));
    const name = this.queues.byId(queue)?.name ?? 'the queue';
    if (ok)
      this.toasts.notify(
        ids.length === 1 ? `Moved to ${name}.` : `${ids.length} downloads moved to ${name}.`,
      );
  }

  /**
   * Moves `ids` inside their queue so they sit just before `before` (or at the end when it is
   * `null`). Only downloads of that one queue take part.
   */
  async placeInQueue(queue: QueueId, ids: DownloadId[], before: DownloadId | null): Promise<void> {
    const next = placed(this.downloads.idsInQueue(queue), ids, before);
    if (next === null) return;
    if (await this.#runner.succeeds(() => this.backend.reorder(queue, next))) {
      this.downloads.reorder(queue, next);
    }
  }

  /** Alt+Up and Alt+Down: nudge the selection one place within its queue. */
  async nudge(delta: -1 | 1): Promise<void> {
    if (this.view.kind !== 'queue' || this.sort !== 'queue') {
      this.toasts.notify('Open a queue in the sidebar to change its order.');
      return;
    }
    const queue = this.view.id;
    const move = nudged(this.downloads.idsInQueue(queue), (id) => this.selection.has(id), delta);
    if (move) await this.placeInQueue(queue, move.chosen, move.before);
  }

  async pauseAll(): Promise<void> {
    await this.#runner.run(() => this.backend.pauseAll());
  }

  async resumeAll(): Promise<void> {
    await this.#runner.run(() => this.backend.resumeAll());
  }

  // ---- Settings ----

  /** Changes settings right away and saves only what changed; on failure those settings come back. */
  updateSettings(change: (draft: AppSettings) => void): Promise<void> {
    return changeSettings(this, change, this.#settingsWriter);
  }

  /** Whether each browser file shows its window before it starts (D23). */
  async setAskBeforeDownload(ask: boolean): Promise<void> {
    await this.updateSettings((s) => {
      s.extension.ask_before_download = ask;
    });
  }

  /** Whether a download's window stays to show its progress after Download now (D23). */
  async setShowProgressWindows(show: boolean): Promise<void> {
    await this.updateSettings((s) => {
      s.show_progress_windows = show;
    });
  }

  /** Tries saving again after a storage failure: an empty change still goes through the app's save. */
  async retryStorage(): Promise<void> {
    if (!this.settings) return;
    const ok = await this.#settingsWriter.write({});
    if (ok) this.storageProblem = null;
  }

  dismissStorageProblem(): void {
    this.storageProblem = null;
  }
}
