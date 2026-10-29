// The download complete window (D52): what finished or failed since it opened, newest group
// first, with Open, Show in folder and Try again. It follows the rows it lists from the core's
// events, and never closes by itself: the person chose it to see what finished when they come
// back.

import { PRODUCT_NAME } from '@wirecatch/product';
import type {
  AppSettings,
  DownloadId,
  DownloadSummary,
  FinishedGroup,
  UiEvent,
} from '@wirecatch/protocol';
import type { Backend, FinishedSource } from '../backend';
import { runningAppName } from '../state/app-name';
import { dispatch, HeldEvents, ignore, type UiEventHandlers } from '../state/events';
import { errorMessage, Runner } from '../state/runner';

/** What the window needs from whatever shows it: a real window, or the sample page. */
export interface FinishedWindowHost {
  backend: () => Backend;
  /** What the window lists: the shell's, or the sample page's. */
  finished: () => FinishedSource;
  /** Closes the window. */
  close: () => void;
}

/** Makes the download complete window's state over a backend the caller does not hand out. */
export type OpenFinishedWindow = (host: Pick<FinishedWindowHost, 'close'>) => FinishedWindowState;

export class FinishedWindowState {
  readonly #host: FinishedWindowHost;
  /** A failed action says why in the window, until the next action starts. */
  readonly #runner = new Runner(
    (text) => (this.error = text),
    () => (this.error = null),
  );

  /** What it lists, the newest group first. */
  groups = $state<FinishedGroup[]>([]);
  settings = $state<AppSettings | null>(null);
  /** The running app's name, for the window title. */
  name = $state(PRODUCT_NAME);
  /** Loaded, or failed to load (`error` says why). */
  loaded = $state(false);
  /** The last action that failed, in plain words. */
  error = $state<string | null>(null);

  #unsubscribe: (() => void)[] = [];
  /** Bumped by every init and dispose, so a superseded init lets go of what it set up. */
  #generation = 0;

  constructor(host: FinishedWindowHost) {
    this.#host = host;
  }

  /** Every download listed, newest first. */
  rows = $derived(this.groups.flatMap((g) => g.downloads));

  /** How many listed downloads finished and failed, as they are now. */
  counts = $derived.by(() => {
    let completed = 0;
    let failed = 0;
    for (const row of this.rows) {
      if (row.status === 'completed') completed += 1;
      else if (row.status === 'failed') failed += 1;
    }
    return { completed, failed };
  });

  /** Loads what the window lists and starts following it. */
  async init(): Promise<void> {
    this.#teardown();
    const generation = ++this.#generation;
    const backend = this.#host.backend();
    const finished = this.#host.finished();
    const events = new HeldEvents((e) => this.handle(e));
    // One at a time, kept as they start, so a listener that fails to start leaves the ones before it to stop.
    this.#unsubscribe = [];
    const stops = this.#unsubscribe;
    try {
      stops.push(await backend.subscribe(events.receive, () => void this.init()));
      stops.push(await finished.onFinished((list) => (this.groups = list.groups)));
      if (generation !== this.#generation) {
        // Closed (or loaded again) while subscribing: nothing would ever close these listeners.
        for (const stop of stops) stop();
        events.drop();
        return;
      }
      const [list, settings, name] = await Promise.all([
        finished.finishedList(),
        backend.getSettings(),
        runningAppName(backend),
      ]);
      this.groups = list.groups;
      this.settings = settings;
      this.name = name;
    } catch (error) {
      this.error = errorMessage(error);
    } finally {
      this.loaded = true;
      events.release();
    }
  }

  dispose(): void {
    this.#generation += 1;
    this.#teardown();
  }

  #teardown(): void {
    for (const stop of this.#unsubscribe) stop();
    this.#unsubscribe = [];
  }

  /** Puts `change` into every group's copy of the rows it touches. */
  #update(change: (row: DownloadSummary) => DownloadSummary | null): void {
    this.groups = this.groups
      .map((group) => ({
        ...group,
        downloads: group.downloads.flatMap((row) => change(row) ?? []),
      }))
      .filter((group) => group.downloads.length > 0);
  }

  /** What this window keeps from each core event: the rows it lists, and the settings. */
  readonly #handlers: UiEventHandlers = {
    downloads_changed: (e) => {
      // A batch holds only the rows that changed, and the window lists few.
      const changed = (id: DownloadId) => e.data.find((row) => row.id === id);
      if (this.rows.every((row) => !changed(row.id))) return;
      this.#update((row) => changed(row.id) ?? row);
    },
    downloads_removed: (e) => {
      if (this.rows.every((row) => !e.data.includes(row.id))) return;
      this.#update((row) => (e.data.includes(row.id) ? null : row));
    },
    settings_changed: (e) => {
      this.settings = e.data;
    },
    queues_changed: ignore,
    proxy_servers_changed: ignore,
    addons_changed: ignore,
    suffix_list_checked: ignore,
    media_analysing: ignore,
    media_choice_requested: ignore,
    addon_required: ignore,
    confirmation_closed: ignore,
    captured: ignore,
    confirmation_requested: ignore,
    settings_imported: ignore,
    storage_failed: ignore,
    window_requested: ignore,
    browser_settings_requested: ignore,
    // What the shell adds arrives on its own event.
    queue_finished: ignore,
    // Between the shell and the browsers (D65).
    notice_shown: ignore,
    notice_pressed: ignore,
    // The shell tells it in a note (D67); its download arrives in the list when it starts.
    media_captured: ignore,
  };

  handle(event: UiEvent): void {
    dispatch(this.#handlers, event);
  }

  open(id: DownloadId): Promise<void> {
    return this.#runner.run(() => this.#host.backend().openFile(id));
  }

  showInFolder(id: DownloadId): Promise<void> {
    return this.#runner.run(() => this.#host.backend().openFolder(id));
  }

  retry(id: DownloadId): Promise<void> {
    return this.#runner.run(() => this.#host.backend().retry(id));
  }

  /** "Use notifications instead": switches the setting, then the window goes. */
  async useNotifications(): Promise<void> {
    const ok = await this.#runner.succeeds(() =>
      this.#host.backend().changeSettings({ notify_on_finish: 'notification' }),
    );
    if (ok) this.close();
  }

  close(): void {
    this.#host.close();
  }
}
