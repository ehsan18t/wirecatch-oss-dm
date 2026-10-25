// Everything the main window knows and every action it can take. Components read from here and
// call these methods; nothing else talks to the backend. Each concern has its own class (the
// link to the backend, the actions on downloads, the selection, the queues, the notes, the
// questions, the add-ons, settings files); this one owns the list, the screens and dialogs, the
// settings and the core's events.

import { PRODUCT_NAME } from '@wirecatch/product';
import type { AppSettings, DownloadId, QueueId, UiEvent } from '@wirecatch/protocol';
import { createContext, untrack } from 'svelte';
import { SvelteSet } from 'svelte/reactivity';
import type { Backend, SampleData } from '../backend';
import { extractUrls } from '../format';
import { SETTINGS, type SettingId } from '../settings-schema';
import type { SectionId } from '../settings-sections';
import { sampleWindows } from '../window/sample-factories';
import { AddDraft } from './add-draft.svelte';
import { Addons } from './addons.svelte';
import { Appearance } from './appearance.svelte';
import { SuffixList } from './suffix-list.svelte';
import { runningAppName } from './app-name';
import { Clock } from './clock.svelte';
import { Connection } from './connection';
import { DownloadActions } from './download-actions.svelte';
import { DownloadStore } from './downloads.svelte';
import { mainWindowEvents } from './app-events';
import { dispatch, HeldEvents } from './events';
import { ProxyForm } from './proxy-form.svelte';
import { Questions } from './questions.svelte';
import { revealDownloads } from './reveal';
import { Queues } from './queues.svelte';
import { errorMessage, Runner } from './runner';
import type { Dialog, Screen } from './screens';
import { Selection } from './selection.svelte';
import { changeSettings, SettingsWriter } from './settings-changes.svelte';
import { SettingsTransfer } from './settings-transfer.svelte';
import { Toasts } from './toasts.svelte';
import { WindowFrameControls } from './window-frame';
import {
  countRows,
  FOLDED_GROUPS,
  groupOf,
  isGrouped,
  listLayout,
  placesOf,
  QUEUE_ORDER,
  type Sort,
  type StatusGroup,
  type View,
} from './view';

export type { Dialog, Screen } from './screens';
export type { Toast } from './toasts.svelte';

const [appContext, setAppContext, hasAppContext] = createContext<AppState>();

export function setApp(app: AppState): void {
  setAppContext(app);
}

export function getApp(): AppState {
  if (!hasAppContext())
    throw new Error('AppState is missing; wrap the component in the app shell.');
  return appContext();
}

export class AppState {
  readonly #connection: Connection;
  readonly downloads = new DownloadStore();
  readonly toasts = new Toasts();
  /** The time schedule labels follow. */
  readonly clock = new Clock();
  readonly #runner = new Runner((text) => this.toasts.fail(text));
  readonly selection = new Selection({
    visible: () => this.visible,
    row: (id) => this.downloads.get(id),
  });
  readonly questions = new Questions({ backend: () => this.#backend });
  /** What the title bar's caption buttons call: this window, through the backend once it has loaded. */
  readonly windowControls = new WindowFrameControls(() => this.#connection.load());
  /** The window's theme and accent on its page (D36); the shell components apply the settings to it. */
  readonly appearance = new Appearance();
  #stopAppearance: (() => void) | null = null;
  readonly addons = new Addons({
    backend: () => this.#backend,
    runner: this.#runner,
    toasts: this.toasts,
  });
  readonly suffixList = new SuffixList({
    backend: () => this.#backend,
    runner: this.#runner,
    toasts: this.toasts,
  });
  /** The proxy setting as edited, saved as a whole (D71). */
  readonly proxy = new ProxyForm({
    backend: () => this.#backend,
    runner: this.#runner,
    saved: () => this.settings?.proxy ?? null,
    running: () => this.counts.status.active - this.counts.joining,
  });
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
      if (settings) this.settingsArrived(settings);
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
    openSettings: (focus) => this.openSettings(focus),
    sort: () => this.sort,
    videoSites: () => this.settings?.extension.video_sites ?? [],
    videosAsk: () => this.settings?.media.ask_each_time ?? true,
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
  /** The settings section shown, one at a time. */
  settingsSection = $state<SectionId>('general');
  /** What the settings are searched for, across every section; empty shows the section. */
  settingsQuery = $state('');
  view = $state<View>({ kind: 'all' });
  search = $state('');
  sort = $state<Sort>(QUEUE_ORDER);
  /** Groups of the list folded under their heading. */
  readonly folded = new SvelteSet<StatusGroup>(FOLDED_GROUPS);

  /** The download whose details show, in a dialog over the list. */
  details = $state<DownloadId | null>(null);

  dialog = $state<Dialog | null>(null);
  /** The Add dialog's form, while it is open. */
  addDraft = $state.raw<AddDraft | null>(null);
  paletteOpen = $state(false);
  /** Saving the download list failed; shown in a banner until dealt with. */
  storageProblem = $state<string | null>(null);
  /** True when the interface runs on sample data instead of the real engine. */
  sampleData = $state(false);
  /** The running app's name, for titles and labels: the backend's, the product name until it answers. */
  name = $state(PRODUCT_NAME);

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
   * The states of the sample page's download windows and its download complete window (D23,
   * D52). The page shows them inside itself, so it needs them made over the backend that only
   * this state holds.
   */
  readonly sampleWindows = sampleWindows(() => this.#backend);

  /** The sample backend's extras (its palette actions and download windows); null on the engine. */
  get sample(): SampleData | null {
    // Read first so the shell follows it: the backend itself is not reactive.
    if (!this.sampleData) return null;
    return this.#connection.loaded?.sampleData ?? null;
  }

  // ---- Derived views ----

  readonly #get = (id: DownloadId) => this.downloads.get(id);

  /** The list's groups and rows. Rebuilt only when membership, view, search, sort, the folded groups or the queues change. */
  layout = $derived.by(() => {
    const order = this.downloads.order;
    const options = {
      view: this.view,
      search: this.search,
      sort: this.sort,
      folded: [...this.folded],
      queues: this.queues.list.map((q) => q.id),
    };
    return untrack(() => listLayout(order, this.#get, options));
  });

  /** Ids the list draws, in display order. */
  visible = $derived(this.layout.ids);

  /** Where each download stands in its queue. */
  places = $derived.by(() => {
    const order = this.downloads.order;
    return untrack(() => placesOf(order, this.#get));
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
    this.#connection.takeReveal(current, (reveal) => revealDownloads(this, reveal));
    this.ready = true;
    this.clock.start();
    // None is needed for the list, so none holds it up.
    void this.#followSystem(current);
    void this.#takeStartupNotices();
    void this.addons.load();
    void this.suffixList.load();
    void this.#takeLaunchLinks();
    void this.questions.restore();
  }

  /** Loads the backend and listens to it; false when superseded or when that failed, which is then shown. */
  async #connect(current: () => boolean, onEvent: (e: UiEvent) => void): Promise<boolean> {
    try {
      const backend = await this.#connection.load();
      this.sampleData = backend.kind === 'mock';
      this.clock.follow(backend.sampleData?.now);
      return await this.#connection.listen(
        current,
        onEvent,
        (signal) => {
          // Events were missed: load everything again rather than show a stale list.
          if (signal === 'resync') void this.init();
          else void this.#takeLaunchLinks();
        },
        (reveal) => revealDownloads(this, reveal),
      );
    } catch (error) {
      if (current()) {
        this.loadError = errorMessage(error);
        this.ready = true;
      }
      return false;
    }
  }

  /** Loads the list, the queues, the settings and the waiting browser files. */
  async #load(current: () => boolean): Promise<void> {
    try {
      const [downloads, queues, settings, pending, name] = await Promise.all([
        this.#backend.listDownloads(),
        this.#backend.listQueues(),
        this.#backend.getSettings(),
        this.#backend.pendingConfirmations(),
        runningAppName(this.#backend),
      ]);
      if (!current()) return;
      this.downloads.load(downloads);
      this.queues.list = queues;
      this.settingsArrived(settings);
      this.questions.pending = pending;
      this.name = name;
      this.loadError = null;
    } catch (error) {
      if (current()) this.loadError = errorMessage(error);
    }
  }

  /** Follows the system's light or dark and accent colour until the next init or dispose. */
  async #followSystem(current: () => boolean): Promise<void> {
    const stop = await this.appearance.connect(this.#backend);
    if (current()) this.#stopAppearance = stop;
    else stop();
  }

  /** How the app started, when the person must know: the system notification may be long gone, or off (CORE-1). */
  async #takeStartupNotices(): Promise<void> {
    const notices = await this.#backend.startupNotices().catch(() => []);
    for (const notice of notices) this.toasts.notice(notice, () => void this.#showSetAsideList());
  }

  async #showSetAsideList(): Promise<void> {
    try {
      await this.#backend.showSetAsideList();
    } catch (error) {
      this.toasts.fail(errorMessage(error));
    }
  }

  /** Links from the command line (`wirecatch-app https://...`) open the Add dialog, or join it when it is open. */
  async #takeLaunchLinks(): Promise<void> {
    let links: string[];
    try {
      links = await this.#backend.takeLaunchLinks();
    } catch {
      return;
    }
    if (links.length === 0) return;
    if (this.addDraft) this.addDraft.append(links);
    else this.openDialog({ kind: 'add', urls: links });
  }

  #teardown(): void {
    this.#connection.close();
    this.clock.stop();
    this.#stopAppearance?.();
    this.#stopAppearance = null;
  }

  dispose(): void {
    this.#generation += 1;
    this.#teardown();
    this.toasts.dispose();
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
    this.details = null;
  }

  get detailsOpen(): boolean {
    return this.details !== null;
  }

  /** Shows the details of `id`, else of the keyboard row or the first selected download; it stays selected, and its group opens. */
  openDetails(id?: DownloadId): void {
    const target = id ?? this.selection.active ?? this.selection.list[0];
    if (target === undefined) return;
    if (this.selection.has(target)) this.selection.active = target;
    else this.selection.only(target);
    const row = this.downloads.get(target);
    if (row && isGrouped(this.view)) this.folded.delete(groupOf(row.status));
    this.screen = 'downloads';
    this.paletteOpen = false;
    this.details = target;
  }

  closeDetails(): void {
    this.details = null;
  }

  /** Folds a group of the list under its heading, or opens it. */
  toggleGroup(group: StatusGroup): void {
    if (this.folded.has(group)) this.folded.delete(group);
    else this.folded.add(group);
  }

  openSettings(focus: SettingId | null = null): void {
    this.screen = 'settings';
    this.settingsFocus = focus;
    if (focus !== null) {
      this.settingsQuery = '';
      this.settingsSection = SETTINGS.find((s) => s.id === focus)?.section ?? this.settingsSection;
    }
    this.paletteOpen = false;
  }

  /** Shows one settings section, leaving any search. */
  openSettingsSection(section: SectionId): void {
    this.screen = 'settings';
    this.settingsQuery = '';
    this.settingsSection = section;
  }

  openDialog(dialog: Dialog): void {
    this.paletteOpen = false;
    this.dialog = dialog;
    this.addDraft = dialog.kind === 'add' ? this.#addDraft(dialog.urls, dialog.queue) : null;
  }

  /** A new Add form with `urls`, for `queue`, else the queue on screen, else the default one. */
  #addDraft(urls: string[], queue: QueueId | undefined): AddDraft {
    const onScreen = this.view.kind === 'queue' ? this.view.id : undefined;
    const draft: AddDraft = new AddDraft(
      {
        add: (requests) => this.actions.add(requests),
        pickFolder: (current) => this.#backend.pickFolder(current),
        added: (ids) => {
          const [first] = ids;
          if (first !== undefined) this.selection.only(first);
        },
        close: () => {
          if (this.addDraft === draft) this.closeDialog();
        },
      },
      { urls, queue: queue ?? onScreen ?? this.queues.defaultQueue?.id ?? null },
    );
    return draft;
  }

  closeDialog(): void {
    this.dialog = null;
    this.addDraft = null;
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

  /** A caption button's call to the window failed: the window stays as it was, and the person learns why. */
  windowFailed(error: unknown): void {
    this.toasts.fail(`The window did not respond: ${errorMessage(error)}`);
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

  /** The app's settings as the core has them, loaded or changed; the proxy being edited follows unless it is being edited. */
  settingsArrived(settings: AppSettings): void {
    this.settings = settings;
    this.proxy.savedChanged();
  }

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
