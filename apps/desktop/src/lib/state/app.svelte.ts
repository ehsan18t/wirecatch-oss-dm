// Everything the interface knows and every action it can take. Components read from here and
// call these methods; nothing else talks to the backend.

import type {
  AddonInfo,
  AddRequest,
  AppSettings,
  CaptureConfirmation,
  DownloadId,
  DownloadSummary,
  FixAction,
  ImportSummary,
  MediaChoice,
  MediaSelection,
  PendingCapture,
  Queue,
  QueueId,
  UiEvent,
} from '@wirecatch/protocol';
import { getContext, setContext, untrack } from 'svelte';
import { SvelteSet } from 'svelte/reactivity';
import { isAddonRequired, isInstalled, MEDIA_ADDON } from '../addons';
import type { Backend } from '../backend/types';
import { extractUrls, formatTime, hostOf, scheduleAllows } from '../format';
import type { SettingId } from '../settings-schema';
import { DownloadStore } from './downloads.svelte';
import { Hints } from './hints.svelte';
import { countRows, visibleIds, type SortKey, type View } from './view';

export type Screen = 'downloads' | 'settings';

/**
 * The add-on prompt: what needs the add-on and the id of what waits for it in the core, a page
 * waiting for the add-on or a quality choice whose picked quality needs it. `url` may be empty.
 */
export interface AddonPrompt {
  addon: AddonInfo;
  url: string;
  id: number;
}

/** A question the core asked, as shown on screen. `waiting` counts the ones queued behind it. */
export type Question =
  | { kind: 'capture'; capture: PendingCapture; waiting: number }
  | { kind: 'addon'; request: AddonPrompt }
  | { kind: 'media'; choice: MediaChoice; waiting: number };

export type Dialog =
  | { kind: 'add'; urls: string[]; queue?: QueueId }
  | { kind: 'remove'; ids: DownloadId[] }
  | { kind: 'cancel'; ids: DownloadId[] }
  | { kind: 'queue'; id: QueueId | null }
  | { kind: 'refresh'; id: DownloadId }
  | { kind: 'export' }
  | { kind: 'import' }
  | { kind: 'shortcuts' }
  | { kind: 'open-program'; id: DownloadId };

export interface Toast {
  id: number;
  kind: 'info' | 'error' | 'capture';
  message: string;
  downloadId?: DownloadId;
}

const CONTEXT = Symbol('wirecatch-app');

export function setApp(app: AppState): void {
  setContext(CONTEXT, app);
}

export function getApp(): AppState {
  const app = getContext<AppState | undefined>(CONTEXT);
  if (!app) throw new Error('AppState is missing; wrap the component in the app shell.');
  return app;
}

function message(err: unknown): string {
  if (err instanceof Error && err.message) return err.message;
  if (typeof err === 'string' && err) return err;
  return 'Something went wrong. Try again.';
}

/** Says what failed in a bulk action, once, instead of one note per download. */
export function bulkFailureMessage(
  verb: string,
  failed: number,
  total: number,
  first: string,
): string {
  if (total === 1) return first;
  return `Could not ${verb} ${failed} of ${total} downloads. ${first}`;
}

export class AppState {
  #backend: Backend | null = null;
  readonly #loadBackend: () => Promise<Backend>;
  readonly downloads = new DownloadStore();
  readonly hints = new Hints();

  queues = $state<Queue[]>([]);
  settings = $state<AppSettings | null>(null);
  ready = $state(false);
  loadError = $state<string | null>(null);

  screen = $state<Screen>('downloads');
  /** The setting to scroll to and focus when the settings screen opens. */
  settingsFocus = $state<SettingId | null>(null);
  view = $state<View>({ kind: 'all' });
  search = $state('');
  sort = $state<SortKey>('queue');

  readonly selection = new SvelteSet<DownloadId>();
  /** The row the keyboard is on. */
  active = $state<DownloadId | null>(null);
  #anchor: DownloadId | null = null;

  /** The window is wide enough for the detail panel to have its own column. */
  wide = $state(true);
  /** In a wide window, whether the detail panel column is shown. */
  detailsOpen = $state(true);

  dialog = $state<Dialog | null>(null);
  paletteOpen = $state(false);
  toasts = $state<Toast[]>([]);
  pending = $state<PendingCapture[]>([]);
  /** The ask-first dialog was closed without an answer; the downloads still wait. */
  pendingHidden = $state(false);
  /** Media found with several qualities, waiting for the person to pick one. */
  mediaChoices = $state<MediaChoice[]>([]);
  /** The quality picker was put aside; the choices still wait. */
  mediaHidden = $state(false);
  /** Saving the download list failed; shown in a banner until dealt with. */
  storageProblem = $state<string | null>(null);
  /** Add-ons and their state; `null` until loaded. */
  addons = $state<AddonInfo[] | null>(null);
  /** Why the add-ons could not be listed, shown in their settings section. */
  addonsError = $state<string | null>(null);
  /** A capture that needs an add-on; the prompt offers to install it. */
  addonRequest = $state<AddonPrompt | null>(null);
  /** The add-on prompt was put aside; what waits for the add-on still waits. */
  addonHidden = $state(false);
  /**
   * A quality the person picked that needs the media add-on first: it downloads by itself once
   * the add-on is installed, and its picker stays out of the way meanwhile.
   */
  choiceAwaitingAddon = $state<{ id: number; selection: MediaSelection } | null>(null);
  /** What the last settings import changed, shown in the import dialog. */
  lastImport = $state<ImportSummary | null>(null);
  /** True when the interface runs on sample data instead of the real engine. */
  sampleData = $state(false);
  /** Refreshed every half minute so schedule labels stay true. */
  // Replaced whole, never mutated, so a plain Date is enough.
  // eslint-disable-next-line svelte/prefer-svelte-reactivity
  now = $state(new Date());

  #toastId = 1;
  /** Questions this window answered; their `confirmation_closed` needs no note. */
  // Bookkeeping only: nothing renders from it, so it need not be reactive.
  // eslint-disable-next-line svelte/prefer-svelte-reactivity
  readonly #answering = new Set<number>();
  #unsubscribe: (() => void) | null = null;
  #clock: ReturnType<typeof setInterval> | undefined;
  /** Where "now" comes from: the system clock, or the sample clock of the mock backend. */
  #clockSource: () => number = () => Date.now();
  /** Bumped by every init and dispose, so a superseded init cleans up after itself. */
  #generation = 0;

  /** Takes a backend, or a function that loads one (so the mock can stay out of production). */
  constructor(backend: Backend | (() => Promise<Backend>)) {
    if (typeof backend === 'function') {
      this.#loadBackend = backend;
    } else {
      this.#backend = backend;
      this.#loadBackend = async () => backend;
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
    const view = this.view;
    const search = this.search;
    const sort = this.sort;
    return untrack(() => visibleIds(order, this.#get, view, search, sort));
  });

  counts = $derived.by(() => {
    const order = this.downloads.order;
    return untrack(() => countRows(order, this.#get));
  });

  selectedRows = $derived.by(() => {
    const rows: DownloadSummary[] = [];
    for (const id of this.selection) {
      const row = this.downloads.get(id);
      if (row) rows.push(row);
    }
    return rows;
  });

  defaultQueue = $derived(this.queues.find((q) => q.is_default) ?? this.queues[0]);

  /** Media choices that wait for the person; one that waits for the add-on is already answered. */
  openMediaChoices = $derived(
    this.mediaChoices.filter((c) => c.id !== this.choiceAwaitingAddon?.id),
  );

  /**
   * The question to show now, one at a time and never over another dialog: a browser download
   * to confirm first, then an add-on to install (it may be what a picked quality waits for),
   * then a quality to pick.
   */
  question = $derived.by((): Question | null => {
    if (this.dialog !== null) return null;
    const capture = this.pendingHidden ? undefined : this.pending[0];
    if (capture) return { kind: 'capture', capture, waiting: this.pending.length - 1 };
    if (this.addonRequest && !this.addonHidden)
      return { kind: 'addon', request: this.addonRequest };
    const choice = this.mediaHidden ? undefined : this.openMediaChoices[0];
    if (choice) return { kind: 'media', choice, waiting: this.openMediaChoices.length - 1 };
    return null;
  });

  queueById(id: QueueId): Queue | undefined {
    return this.queues.find((q) => q.id === id);
  }

  /** True when a scheduled queue is outside its hours and not started by hand. */
  isOutsideSchedule(queue: Queue): boolean {
    return (
      queue.schedule !== null &&
      !queue.running_outside_schedule &&
      !scheduleAllows(queue.schedule, this.now)
    );
  }

  /** A short phrase for the sidebar: what this queue is doing right now. */
  queueStatus(queue: Queue): string {
    const c = this.counts.queue.get(queue.id);
    if (queue.paused) return 'Paused';
    if (queue.schedule && this.isOutsideSchedule(queue)) {
      return `Starts at ${formatTime(queue.schedule.start)}`;
    }
    if (queue.running_outside_schedule) return 'Started by hand, outside its hours';
    if (c && c.running > 0) return `${c.running} of ${queue.max_concurrent} running`;
    if (c && c.waiting > 0) return `${c.waiting} waiting`;
    if (c && c.paused > 0) return `${c.paused} paused`;
    return 'Idle';
  }

  // ---- Startup and events ----

  /**
   * Loads everything and starts listening. Safe to call again (the Try again button does): the
   * previous listener and clock are torn down first, and an init that is overtaken by a newer
   * init or by `dispose` drops what it set up.
   */
  async init(): Promise<void> {
    this.#teardown();
    const generation = ++this.#generation;
    const buffered: UiEvent[] = [];
    let loading = true;
    let backend: Backend;
    let unsubscribe: () => void;
    try {
      backend = this.#backend ?? (await this.#loadBackend());
      this.#backend = backend;
      this.sampleData = backend.kind === 'mock';
      this.#clockSource = backend.sampleData?.now ?? (() => Date.now());
      // Replaced whole, never mutated, so a plain Date is enough.
      // eslint-disable-next-line svelte/prefer-svelte-reactivity
      this.now = new Date(this.#clockSource());
      const stopEvents = await backend.subscribe((e) => {
        if (loading) buffered.push(e);
        else this.handle(e);
      });
      const stopSignals = await backend.onShellSignal((signal) => {
        if (generation !== this.#generation) return;
        // Events were missed: load everything again rather than show a stale list.
        if (signal === 'resync') void this.init();
        else void this.#takeLaunchLinks();
      });
      unsubscribe = () => {
        stopEvents();
        stopSignals();
      };
    } catch (err) {
      if (generation === this.#generation) {
        this.loadError = message(err);
        this.ready = true;
      }
      return;
    }
    if (generation !== this.#generation) {
      unsubscribe();
      return;
    }
    this.#unsubscribe = unsubscribe;
    try {
      const [downloads, queues, settings, pending] = await Promise.all([
        this.backend.listDownloads(),
        this.backend.listQueues(),
        this.backend.getSettings(),
        this.backend.pendingConfirmations(),
      ]);
      if (generation !== this.#generation) return;
      this.downloads.load(downloads);
      this.queues = queues;
      this.settings = settings;
      this.pending = pending;
      this.loadError = null;
    } catch (err) {
      if (generation === this.#generation) this.loadError = message(err);
    } finally {
      loading = false;
      if (generation === this.#generation) {
        for (const e of buffered) this.handle(e);
        this.ready = true;
      }
    }
    if (generation !== this.#generation) return;
    // eslint-disable-next-line svelte/prefer-svelte-reactivity
    this.#clock = setInterval(() => (this.now = new Date(this.#clockSource())), 30_000);
    // Neither is needed for the list, so neither holds it up.
    void this.loadAddons();
    void this.#takeLaunchLinks();
    void this.#restoreQuestions();
  }

  /**
   * Questions asked while no window was open, or before a reload: open media choices and a
   * waiting add-on prompt. Confirmations come back with the main load.
   */
  async #restoreQuestions(): Promise<void> {
    try {
      const choices = await this.backend.pendingMediaChoices();
      const fresh = choices.filter((c) => !this.mediaChoices.some((k) => k.id === c.id));
      if (fresh.length > 0) this.mediaChoices = [...this.mediaChoices, ...fresh];
    } catch {
      // The events still bring new choices.
    }
    try {
      // Pages that wait for the add-on: offer the install again, for the oldest one.
      const [waiting] = await this.backend.pendingAddonRequests();
      if (waiting && !this.addonRequest) this.addonRequest = { ...waiting };
    } catch {
      // Same: the event still brings a new prompt.
    }
  }

  /** Lists the add-ons. A failure is shown in their settings section, not as an error note. */
  async loadAddons(): Promise<void> {
    try {
      this.addons = await this.backend.listAddons();
      this.addonsError = null;
    } catch (err) {
      this.addonsError = message(err);
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
    if (this.#clock) clearInterval(this.#clock);
    this.#clock = undefined;
  }

  dispose(): void {
    this.#generation += 1;
    this.#teardown();
  }

  handle(event: UiEvent): void {
    switch (event.type) {
      case 'downloads_changed':
        this.downloads.apply(event.data);
        break;
      case 'downloads_removed': {
        this.downloads.remove(event.data);
        for (const id of event.data) this.selection.delete(id);
        if (this.active !== null && event.data.includes(this.active)) this.active = null;
        break;
      }
      case 'queues_changed':
        this.queues = event.data;
        if (this.view.kind === 'queue') {
          const id = this.view.id;
          if (!event.data.some((q) => q.id === id)) this.view = { kind: 'all' };
        }
        break;
      case 'settings_changed':
        this.settings = event.data;
        break;
      case 'captured':
        this.downloads.apply([event.data]);
        this.#toast({ kind: 'capture', message: event.data.file_name, downloadId: event.data.id });
        break;
      case 'confirmation_requested':
        if (!this.pending.some((p) => p.id === event.data.id)) {
          this.pending = [...this.pending, event.data];
        }
        // A new question always shows, even if an earlier one was put aside.
        this.pendingHidden = false;
        break;
      case 'media_choice_requested':
        if (!this.mediaChoices.some((c) => c.id === event.data.id)) {
          this.mediaChoices = [...this.mediaChoices, event.data];
        }
        // A new question always shows, even if an earlier one was put aside.
        this.mediaHidden = false;
        break;
      case 'addons_changed': {
        this.addons = event.data;
        this.addonsError = null;
        const asked = this.addonRequest;
        const fresh = asked && event.data.find((a) => a.id === asked.addon.id);
        if (asked && fresh) {
          // Once installed, the waiting download carries on by itself; the prompt has done its job.
          if (fresh.state.type === 'installed') {
            this.addonRequest = null;
            this.notify(`${fresh.name} installed. Your download continues.`);
          } else {
            this.addonRequest = { ...asked, addon: fresh };
          }
        }
        const waiting = this.choiceAwaitingAddon;
        const media = event.data.find((a) => a.id === MEDIA_ADDON);
        if (waiting && isInstalled(media)) {
          this.choiceAwaitingAddon = null;
          void this.chooseMedia(waiting.id, waiting.selection);
        } else if (waiting && media?.state.type === 'not_installed') {
          // The install was stopped: nothing will come, so the picker comes back.
          this.choiceAwaitingAddon = null;
          if (this.addonRequest?.id === waiting.id) this.closeAddonRequest();
          this.mediaHidden = false;
        }
        break;
      }
      case 'addon_required':
        this.addonRequest = { ...event.data };
        // A new question always shows, even if an earlier one was put aside.
        this.addonHidden = false;
        break;
      case 'confirmation_closed':
        this.#questionClosed(event.data.id);
        break;
      case 'settings_imported':
        this.lastImport = event.data;
        break;
      case 'storage_failed':
        // Not a passing note: it stays until the person deals with it.
        this.storageProblem = event.data.message;
        break;
    }
  }

  // ---- Toasts ----

  #toast(t: Omit<Toast, 'id'>): number {
    const id = this.#toastId++;
    // Keep the stack short; the oldest note goes first.
    this.toasts = [...this.toasts.slice(-3), { ...t, id }];
    return id;
  }

  notify(text: string): void {
    this.#toast({ kind: 'info', message: text });
  }

  fail(text: string): void {
    this.#toast({ kind: 'error', message: text });
  }

  dismissToast(id: number): void {
    this.toasts = this.toasts.filter((t) => t.id !== id);
  }

  /**
   * Runs one action for many downloads at once and reports any failures in a single note.
   * Returns how many succeeded.
   */
  async #bulk(
    ids: DownloadId[],
    verb: string,
    action: (id: DownloadId) => Promise<void>,
  ): Promise<number> {
    if (ids.length === 0) return 0;
    const results = await Promise.allSettled(ids.map((id) => action(id)));
    const failures = results.filter((r): r is PromiseRejectedResult => r.status === 'rejected');
    if (failures.length > 0) {
      this.fail(
        bulkFailureMessage(verb, failures.length, ids.length, message(failures[0]?.reason)),
      );
    }
    return ids.length - failures.length;
  }

  /** Runs a backend action and turns any failure into a readable error note. */
  async #run<T>(action: () => Promise<T>): Promise<T | undefined> {
    try {
      return await action();
    } catch (err) {
      this.fail(message(err));
      return undefined;
    }
  }

  // ---- Navigation ----

  show(view: View): void {
    this.view = view;
    this.screen = 'downloads';
    this.clearSelection();
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
  }

  /** Pasted text: open the Add dialog with every link in it. */
  pasteText(text: string): boolean {
    const urls = extractUrls(text);
    if (urls.length === 0) {
      this.fail(
        'There is no link in what you pasted. Copy the full address, starting with https://.',
      );
      return false;
    }
    this.openDialog({ kind: 'add', urls });
    return true;
  }

  // ---- Selection ----

  selectOnly(id: DownloadId): void {
    this.selection.clear();
    this.selection.add(id);
    this.#anchor = id;
    this.active = id;
  }

  toggle(id: DownloadId): void {
    if (this.selection.has(id)) this.selection.delete(id);
    else this.selection.add(id);
    this.#anchor = id;
    this.active = id;
  }

  /** Selects from the last clicked row to this one, in display order. */
  selectRange(id: DownloadId, additive = false): void {
    const list = this.visible;
    const from = this.#anchor === null ? -1 : list.indexOf(this.#anchor);
    const to = list.indexOf(id);
    if (from < 0 || to < 0) {
      this.selectOnly(id);
      return;
    }
    if (!additive) this.selection.clear();
    const [a, b] = from < to ? [from, to] : [to, from];
    for (let i = a; i <= b; i++) {
      const x = list[i];
      if (x !== undefined) this.selection.add(x);
    }
    this.active = id;
  }

  selectAll(): void {
    for (const id of this.visible) this.selection.add(id);
  }

  clearSelection(): void {
    this.selection.clear();
    this.#anchor = null;
    this.active = null;
  }

  /** Arrow keys: move the keyboard row, optionally extending the selection. */
  moveActive(delta: number, extend: boolean): void {
    const list = this.visible;
    if (list.length === 0) return;
    const current = this.active === null ? -1 : list.indexOf(this.active);
    const next =
      current < 0
        ? delta > 0
          ? 0
          : list.length - 1
        : Math.min(list.length - 1, Math.max(0, current + delta));
    const id = list[next];
    if (id === undefined) return;
    if (extend) this.selectRange(id);
    else this.selectOnly(id);
  }

  get selectedIds(): DownloadId[] {
    return [...this.selection];
  }

  // ---- Download actions ----

  async add(requests: AddRequest[]): Promise<DownloadId[]> {
    const ids: DownloadId[] = [];
    for (const r of requests) {
      const id = await this.#run(() => this.backend.addDownload(r));
      if (id !== undefined) ids.push(id);
    }
    if (ids.length > 0) {
      this.notify(ids.length === 1 ? 'Download added.' : `${ids.length} downloads added.`);
    }
    return ids;
  }

  async pause(ids: DownloadId[] = this.selectedIds): Promise<void> {
    await this.#bulk(ids, 'pause', (id) => this.backend.pause(id));
  }

  async resume(ids: DownloadId[] = this.selectedIds): Promise<void> {
    await this.#bulk(ids, 'resume', (id) => this.backend.resume(id));
  }

  /** Space bar: pause what runs, resume what does not. */
  async togglePause(ids: DownloadId[] = this.selectedIds): Promise<void> {
    const rows = ids.flatMap((id) => this.downloads.get(id) ?? []);
    const running = rows.some(
      (r) =>
        r.status === 'downloading' ||
        r.status === 'connecting' ||
        r.status === 'queued' ||
        r.status === 'processing',
    );
    if (running) {
      await this.pause(ids);
      return;
    }
    // A cancelled download has no partial file left; it starts over.
    const cancelled = rows.filter((r) => r.status === 'cancelled').map((r) => r.id);
    await this.retry(cancelled);
    await this.resume(ids.filter((id) => !cancelled.includes(id)));
  }

  async retry(ids: DownloadId[] = this.selectedIds): Promise<void> {
    await this.#bulk(ids, 'restart', (id) => this.backend.retry(id));
  }

  /** Tries every download that needs attention again. */
  async retryAllFailed(): Promise<void> {
    const failed = this.downloads.order.filter((id) => this.downloads.get(id)?.status === 'failed');
    if (failed.length === 0) {
      this.notify('Nothing needs attention right now.');
      return;
    }
    const ok = await this.#bulk(failed, 'restart', (id) => this.backend.retry(id));
    if (ok > 0)
      this.notify(ok === 1 ? 'Trying 1 download again.' : `Trying ${ok} downloads again.`);
  }

  async cancel(ids: DownloadId[] = this.selectedIds): Promise<void> {
    await this.#bulk(ids, 'cancel', (id) => this.backend.cancel(id));
  }

  async remove(ids: DownloadId[], deleteFile: boolean): Promise<void> {
    await this.#run(() => this.backend.remove(ids, deleteFile));
  }

  async refreshLink(id: DownloadId, url: string): Promise<boolean> {
    const ok = await this.#run(async () => {
      await this.backend.refreshLink(id, url);
      return true;
    });
    return ok === true;
  }

  /** Opens a finished file; a program asks first whether the person trusts where it came from. */
  async openFile(id: DownloadId): Promise<void> {
    const program = await this.#run(() => this.backend.isProgram(id));
    if (program === undefined) return;
    if (program) {
      this.openDialog({ kind: 'open-program', id });
      return;
    }
    await this.#run(() => this.backend.openFile(id));
  }

  /** Opens a program the person confirmed they trust. */
  async openProgram(id: DownloadId): Promise<void> {
    this.closeDialog();
    await this.#run(() => this.backend.openFile(id, true));
  }

  async openFolder(id: DownloadId): Promise<void> {
    await this.#run(() => this.backend.openFolder(id));
  }

  /** Lets the person pick a new folder for one download. */
  async chooseFolderFor(id: DownloadId): Promise<boolean> {
    const row = this.downloads.get(id);
    const picked = await this.#run(() => this.backend.pickFolder(row?.path));
    if (!picked) return false;
    const ok = await this.#run(async () => {
      await this.backend.setDownloadFolder(id, picked);
      return true;
    });
    if (ok) this.notify(`Saving to ${picked}`);
    return ok === true;
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
    }
  }

  async moveToQueue(ids: DownloadId[], queue: QueueId): Promise<void> {
    if (ids.length === 0) return;
    const ok = await this.#run(async () => {
      await this.backend.moveToQueue(ids, queue);
      return true;
    });
    const name = this.queueById(queue)?.name ?? 'the queue';
    if (ok)
      this.notify(
        ids.length === 1 ? `Moved to ${name}.` : `${ids.length} downloads moved to ${name}.`,
      );
  }

  /**
   * Moves `ids` inside their queue so they sit just before `before` (or at the end when it is
   * `null`). Only downloads of that one queue take part.
   */
  async placeInQueue(queue: QueueId, ids: DownloadId[], before: DownloadId | null): Promise<void> {
    const current = this.downloads.idsInQueue(queue);
    const moving = current.filter((id) => ids.includes(id));
    if (moving.length === 0) return;
    const rest = current.filter((id) => !ids.includes(id));
    const at = before === null ? rest.length : rest.indexOf(before);
    const next = [
      ...rest.slice(0, at < 0 ? rest.length : at),
      ...moving,
      ...rest.slice(at < 0 ? rest.length : at),
    ];
    if (next.every((id, i) => id === current[i])) return;
    const ok = await this.#run(async () => {
      await this.backend.reorder(queue, next);
      return true;
    });
    if (ok) this.downloads.reorder(queue, next);
  }

  /** Alt+Up and Alt+Down: nudge the selection one place within its queue. */
  async nudge(delta: -1 | 1): Promise<void> {
    if (this.view.kind !== 'queue' || this.sort !== 'queue') {
      this.notify('Open a queue in the sidebar to change its order.');
      return;
    }
    const queue = this.view.id;
    const current = this.downloads.idsInQueue(queue);
    const chosen = current.filter((id) => this.selection.has(id));
    if (chosen.length === 0) return;
    const first = current.indexOf(chosen[0] as DownloadId);
    const last = current.indexOf(chosen[chosen.length - 1] as DownloadId);
    const rest = current.filter((id) => !this.selection.has(id));
    let before: DownloadId | null;
    if (delta < 0) {
      if (first === 0) return;
      // The row just above the first selected one is never selected itself.
      before = current[first - 1] ?? null;
    } else {
      if (last === current.length - 1) return;
      const after = current[last + 1];
      const i = after === undefined ? -1 : rest.indexOf(after);
      before = i < 0 ? null : (rest[i + 1] ?? null);
    }
    await this.placeInQueue(queue, chosen, before);
  }

  async pauseAll(): Promise<void> {
    await this.#run(() => this.backend.pauseAll());
  }

  async resumeAll(): Promise<void> {
    await this.#run(() => this.backend.resumeAll());
  }

  /** Pauses or resumes a whole queue. Downloads paused one by one stay paused either way. */
  async setQueuePaused(queue: QueueId, paused: boolean): Promise<void> {
    await this.#run(() => this.backend.setQueuePaused(queue, paused));
  }

  /** Runs a scheduled queue now, outside its hours, until it is empty or paused. */
  async startQueueNow(queue: QueueId): Promise<void> {
    const ok = await this.#run(async () => {
      await this.backend.startQueueNow(queue);
      return true;
    });
    const name = this.queueById(queue)?.name ?? 'The queue';
    if (ok) this.notify(`${name} is running now. Its schedule applies again once it is empty.`);
  }

  // ---- Queues ----

  async createQueue(name: string): Promise<Queue | undefined> {
    return this.#run(() => this.backend.createQueue(name));
  }

  async updateQueue(queue: Queue): Promise<boolean> {
    const ok = await this.#run(async () => {
      await this.backend.updateQueue(queue);
      return true;
    });
    return ok === true;
  }

  async deleteQueue(id: QueueId): Promise<boolean> {
    const name = this.queueById(id)?.name;
    const ok = await this.#run(async () => {
      await this.backend.deleteQueue(id);
      return true;
    });
    if (ok)
      this.notify(
        `Deleted ${name ?? 'the queue'}. Its downloads moved to ${this.defaultQueue?.name ?? 'the default queue'}.`,
      );
    return ok === true;
  }

  // ---- Settings ----

  /** Changes settings right away and saves them; on failure the old values come back. */
  async updateSettings(change: (draft: AppSettings) => void): Promise<void> {
    if (!this.settings) return;
    const before = $state.snapshot(this.settings);
    const draft = $state.snapshot(this.settings);
    change(draft);
    this.settings = draft;
    const ok = await this.#run(async () => {
      await this.backend.updateSettings(draft);
      return true;
    });
    if (!ok) this.settings = before;
  }

  async exportSettings(): Promise<string | null> {
    const path = await this.#run(() => this.backend.pickSaveFile('wirecatch-settings.json'));
    if (!path) return null;
    const ok = await this.#run(async () => {
      await this.backend.exportSettings(path);
      return true;
    });
    return ok ? path : null;
  }

  async importSettings(): Promise<string | null> {
    const path = await this.#run(() => this.backend.pickOpenFile());
    if (!path) return null;
    // The core reports what changed with `settings_imported`.
    this.lastImport = null;
    const ok = await this.#run(async () => {
      await this.backend.importSettings(path);
      return true;
    });
    if (!ok) return null;
    // The core also sends change events; reading back makes sure nothing is missed.
    const [settings, queues] = await Promise.all([
      this.#run(() => this.backend.getSettings()),
      this.#run(() => this.backend.listQueues()),
    ]);
    if (settings) this.settings = settings;
    if (queues) this.queues = queues;
    return path;
  }

  // ---- Browser captures ----

  async confirmCapture(id: number, answer: CaptureConfirmation): Promise<void> {
    this.#answering.add(id);
    const ok = await this.#run(async () => {
      await this.backend.confirmCapture(id, answer);
      return true;
    });
    if (ok) this.pending = this.pending.filter((p) => p.id !== id);
  }

  /** Brings back the quality picker after it was put aside. */
  showMediaChoices(): void {
    this.paletteOpen = false;
    this.mediaHidden = false;
  }

  /** Puts the quality picker aside; the choice stays open and a note in the sidebar brings it back. */
  hideMediaChoices(): void {
    this.mediaHidden = true;
  }

  /** Tries saving again after a storage failure: any change is saved, so this saves settings. */
  async retryStorage(): Promise<void> {
    if (!this.settings) return;
    const settings = $state.snapshot(this.settings);
    const ok = await this.#run(async () => {
      await this.backend.updateSettings(settings);
      return true;
    });
    if (ok) this.storageProblem = null;
  }

  dismissStorageProblem(): void {
    this.storageProblem = null;
  }

  /** Brings back the ask-first dialog after it was closed without an answer. */
  showPending(): void {
    this.paletteOpen = false;
    this.pendingHidden = false;
  }

  async rejectCapture(id: number): Promise<void> {
    this.#answering.add(id);
    const ok = await this.#run(async () => {
      await this.backend.rejectCapture(id);
      return true;
    });
    if (ok) this.pending = this.pending.filter((p) => p.id !== id);
  }

  // ---- Media and add-ons ----

  /**
   * Downloads the picked quality; the choice goes away once the core accepts it. A quality that
   * needs the media add-on while it is missing installs the add-on first (the picker says so on
   * its button) and then downloads by itself.
   */
  async chooseMedia(id: number, selection: MediaSelection): Promise<boolean> {
    const choice = this.mediaChoices.find((c) => c.id === id);
    const needsAddon = choice?.options.find((o) => o.id === selection.option_id)?.needs_ffmpeg;
    const media = this.addons?.find((a) => a.id === MEDIA_ADDON);
    if (choice && needsAddon && media && !isInstalled(media)) {
      this.choiceAwaitingAddon = { id, selection };
      this.addonRequest = { addon: media, url: choice.page_url ?? '', id };
      this.addonHidden = false;
      if (media.state.type !== 'installing') void this.installAddon(media.id);
      return true;
    }
    this.#answering.add(id);
    try {
      await this.backend.chooseMedia(id, selection);
    } catch (err) {
      this.#answering.delete(id);
      if (needsAddon && isAddonRequired(message(err))) {
        // The core asked for the add-on instead; its prompt explains, and the choice
        // continues once it is installed.
        this.choiceAwaitingAddon = { id, selection };
        return true;
      }
      this.fail(message(err));
      return false;
    }
    this.mediaChoices = this.mediaChoices.filter((c) => c.id !== id);
    return true;
  }

  async dismissMediaChoice(id: number): Promise<void> {
    // Gone from view at once; the core forgets it in the background.
    this.#answering.add(id);
    this.mediaChoices = this.mediaChoices.filter((c) => c.id !== id);
    await this.#run(() => this.backend.dismissMediaChoice(id));
  }

  /**
   * The core closed a question: answered here or in another window, withdrawn by the browser,
   * or replaced. If it was on screen and not answered here, a note says why it went away.
   */
  #questionClosed(id: number): void {
    const shown = this.question;
    const capture = this.pending.find((p) => p.id === id);
    const choice = this.mediaChoices.find((c) => c.id === id);
    if (capture) this.pending = this.pending.filter((p) => p.id !== id);
    if (choice) this.mediaChoices = this.mediaChoices.filter((c) => c.id !== id);
    if (this.choiceAwaitingAddon?.id === id) this.choiceAwaitingAddon = null;
    const page = this.addonRequest?.id === id ? this.addonRequest : null;
    if (this.#answering.delete(id)) return;
    // The page that asked for the add-on is gone (taken back by the browser, or waited too
    // long). A failed install keeps its prompt, which offers Try again.
    if (page && page.addon.state.type !== 'failed') {
      this.closeAddonRequest();
      this.notify(`The video from ${hostOf(page.url)} no longer waits for the media add-on.`);
      return;
    }
    const onScreen =
      (shown?.kind === 'capture' && shown.capture.id === id) ||
      (shown?.kind === 'media' && shown.choice.id === id);
    const name = capture?.file_name ?? choice?.title;
    if (onScreen && name) {
      this.notify(`${name} was answered in another window, or the browser took it back.`);
    }
  }

  async installAddon(id: string): Promise<boolean> {
    const ok = await this.#run(async () => {
      await this.backend.installAddon(id);
      return true;
    });
    return ok === true;
  }

  async updateAddon(id: string): Promise<void> {
    await this.#run(() => this.backend.updateAddon(id));
  }

  async removeAddon(id: string): Promise<void> {
    await this.#run(() => this.backend.removeAddon(id));
  }

  /** Stops an install or update in progress; an update keeps the installed version. */
  async cancelAddonOperation(id: string): Promise<void> {
    await this.#run(() => this.backend.cancelAddonOperation(id));
  }

  /**
   * Puts the add-on prompt aside (Escape, the close button, a click outside, or Close and keep
   * installing). Nothing is answered: what waits for the add-on still waits, and a note in the
   * sidebar and the palette bring the prompt back.
   */
  hideAddonRequest(): void {
    this.addonHidden = true;
  }

  /** Brings back the add-on prompt after it was put aside. */
  showAddonRequest(): void {
    this.paletteOpen = false;
    this.addonHidden = false;
  }

  /** Closes the prompt once the add-on is installed; there is nothing left to answer. */
  closeAddonRequest(): void {
    this.addonRequest = null;
    this.addonHidden = false;
  }

  /**
   * Not now: the add-on is not wanted for this one. A quality picked here gets its picker back,
   * so the person can pick a quality that does not need the add-on; a page that waited for the
   * add-on is dropped (only that page), so it neither asks again after a reload nor turns into
   * a failed download later.
   */
  declineAddonRequest(): void {
    const asked = this.addonRequest;
    this.closeAddonRequest();
    if (!asked) return;
    if (this.choiceAwaitingAddon?.id === asked.id) {
      this.choiceAwaitingAddon = null;
      return;
    }
    if (this.mediaChoices.some((c) => c.id === asked.id)) return;
    this.#answering.add(asked.id);
    void this.#run(() => this.backend.dismissMediaChoice(asked.id));
  }

  async setAskBeforeDownload(ask: boolean): Promise<void> {
    await this.updateSettings((s) => {
      s.extension.ask_before_download = ask;
    });
  }
}
