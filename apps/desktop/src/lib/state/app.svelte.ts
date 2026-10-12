// Everything the main window knows and every action it can take. Components read from here and
// call these methods; nothing else talks to the backend. Each concern has its own class (the
// link to the backend, the actions on downloads, the selection, the queues, the notes, the
// questions, the add-ons, settings files); this one owns the list, the screens and dialogs, the
// settings and the core's events.

import type { AppSettings, DownloadId, QueueId, UiEvent } from '@wirecatch/protocol';
import { getContext, setContext, untrack } from 'svelte';
import type { Backend, SampleData } from '../backend';
import { extractUrls } from '../format';
import type { SettingId } from '../settings-schema';
import { DownloadWindowState, type OpenDownloadWindow } from '../window/download-window.svelte';
import { Addons } from './addons.svelte';
import { Clock } from './clock.svelte';
import { Connection } from './connection';
import { DownloadActions } from './download-actions.svelte';
import { DownloadStore } from './downloads.svelte';
import { mainWindowEvents } from './app-events';
import { dispatch, HeldEvents } from './events';
import { Hints } from './hints.svelte';
import { Questions } from './questions.svelte';
import { Queues } from './queues.svelte';
import { errorMessage, Runner } from './runner';
import { Selection } from './selection.svelte';
import { changeSettings, SettingsWriter } from './settings-changes.svelte';
import { SettingsTransfer } from './settings-transfer.svelte';
import { Toasts } from './toasts.svelte';
import { countRows, visibleIds, type SortKey, type View } from './view';

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
  readonly #connection: Connection;
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
  readonly addons = new Addons(() => this.#backend, this.#runner, this.toasts);
  readonly queues = new Queues({
    backend: () => this.#backend,
    runner: this.#runner,
    toasts: this.toasts,
    now: () => this.clock.now,
    counts: () => this.counts,
  });
  readonly transfer = new SettingsTransfer({
    backend: () => this.#backend,
    runner: this.#runner,
    imported: (settings, queues) => {
      if (settings) this.settings = settings;
      if (queues) this.queues.list = queues;
    },
  });
  readonly actions = new DownloadActions({
    backend: () => this.#backend,
    runner: this.#runner,
    toasts: this.toasts,
    downloads: this.downloads,
    selection: this.selection,
    queues: this.queues,
    openDialog: (dialog) => this.openDialog(dialog),
    closeDialog: () => this.closeDialog(),
    place: () => ({ view: this.view, sort: this.sort }),
  });
  /** Settings changes go to the app one after another, in the order they were made. */
  readonly #settingsWriter = new SettingsWriter((patch) =>
    this.#runner.succeeds(() => this.#backend.changeSettings(patch)),
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

  /** Bumped by every init and dispose, so a superseded init cleans up after itself. */
  #generation = 0;

  /** Takes a backend, or a function that loads one (so the mock can stay out of production). */
  constructor(backend: Backend | (() => Promise<Backend>)) {
    this.#connection = new Connection(backend);
  }

  /** The backend, once `init` has loaded it. Only state talks to it; components call actions. */
  get #backend(): Backend {
    return this.#connection.backend;
  }

  /**
   * The state of one of the sample page's download windows (D23). The page shows them inside
   * itself, so it needs them made over the backend that only this state holds.
   */
  readonly openSampleWindow: OpenDownloadWindow = (target, host) =>
    new DownloadWindowState(this.#backend, target, host);

  /** The sample backend's extras (its palette actions and download windows); null on the engine. */
  get sample(): SampleData | null {
    // Read first so the shell follows it: the backend itself is not reactive.
    if (!this.sampleData) return null;
    return this.#connection.loaded?.sampleData ?? null;
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
    if (!(await this.#connect(current, events.receive))) return;
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
    void this.questions.restore(this.#backend);
  }

  /** Loads the backend and listens to it; false when superseded or when that failed, which is then shown. */
  async #connect(current: () => boolean, onEvent: (e: UiEvent) => void): Promise<boolean> {
    try {
      const backend = await this.#connection.load();
      this.sampleData = backend.kind === 'mock';
      this.clock.follow(backend.sampleData?.now);
      return await this.#connection.listen(current, onEvent, (signal) => {
        // Events were missed: load everything again rather than show a stale list.
        if (signal === 'resync') void this.init();
        else void this.#takeLaunchLinks();
      });
    } catch (err) {
      if (current()) {
        this.loadError = errorMessage(err);
        this.ready = true;
      }
      return false;
    }
  }

  /** Loads the list, the queues, the settings and the waiting browser files. */
  async #load(current: () => boolean): Promise<void> {
    try {
      const [downloads, queues, settings, pending] = await Promise.all([
        this.#backend.listDownloads(),
        this.#backend.listQueues(),
        this.#backend.getSettings(),
        this.#backend.pendingConfirmations(),
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
      links = await this.#backend.takeLaunchLinks();
    } catch {
      return;
    }
    if (links.length === 0) return;
    const open = this.dialog;
    const urls = open?.kind === 'add' ? [...open.urls, ...links] : links;
    this.openDialog({ kind: 'add', urls });
  }

  #teardown(): void {
    this.#connection.close();
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

  /** Asks for a folder, starting at `current`; null when none was picked or the picker failed. */
  async pickFolder(current?: string): Promise<string | null> {
    return (await this.#runner.run(() => this.#backend.pickFolder(current))) ?? null;
  }

  /** Tells the shell the first view is drawn, so it shows the window; without a backend, nothing. */
  async mainWindowReady(): Promise<void> {
    await this.#connection.loaded?.mainWindowReady().catch(() => undefined);
  }

  /** Brings every waiting question's window to the front, opening closed ones again. */
  async showQuestionWindows(): Promise<void> {
    this.paletteOpen = false;
    await this.#runner.run(() => this.#backend.showQuestionWindows());
  }

  /** Opens (or brings forward) a download's own progress window. */
  async showProgressWindow(id: DownloadId): Promise<void> {
    this.paletteOpen = false;
    await this.#runner.run(() => this.#backend.showProgressWindow(id));
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
