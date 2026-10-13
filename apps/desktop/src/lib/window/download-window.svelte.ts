// Everything one download window knows and does (D23). A window shows one question (confirm a
// browser file, a video being read, pick a quality, install the media add-on) and, after
// Download now, that download's progress. It loads only its own question or download, and of
// each event keeps only what concerns it, so a window costs nothing per download in the list.

import { PRODUCT_NAME } from '@wirecatch/product';
import type {
  AddonInfo,
  AppSettings,
  CaptureConfirmation,
  DownloadId,
  DownloadSummary,
  MediaChoice,
  MediaSelection,
  Question,
  QuestionId,
  Queue,
  UiEvent,
} from '@wirecatch/protocol';
import { isAddonRequired, isInstalled, MEDIA_ADDON } from '../addons';
import type { Backend } from '../backend';
import { Clock } from '../state/clock.svelte';
import { dispatch, HeldEvents, ignore, type UiEventHandlers } from '../state/events';
import { errorMessage, Runner } from '../state/runner';
import { changeSettings, SettingsWriter } from '../state/settings-changes.svelte';
import { ProgressActions } from './progress-actions';
import { runningAppName } from '../state/app-name';

/** What the window is showing. */
export type Phase = 'loading' | 'ask' | 'progress' | 'gone';

/** What the window needs from whatever shows it: a real window, or the sample page. */
export interface DownloadWindowHost {
  backend: () => Backend;
  /** Closes the window. Never answers its question. */
  close(): void;
  /** The window now shows download `id` (only the sample page needs telling). */
  becameDownload?(id: DownloadId): void;
}

/** What the window was opened for. */
export type Target = { question: QuestionId } | { download: DownloadId };

/** Makes a download window's state over a backend the caller does not hand out. */
export type OpenDownloadWindow = (
  target: Target,
  host: Omit<DownloadWindowHost, 'backend'>,
) => DownloadWindowState;

/** The answer in a question window's form. */
export interface Answer {
  fileName: string | null;
  folder: string | null;
  queue: Queue['id'] | null;
}

/** Said when a question closes without an answer from this window. */
export const ANSWERED_ELSEWHERE =
  'This download is no longer waiting. It was answered in another window, or the browser took it back.';

/** Said when a video being read closes without a quality list. */
export const READING_ENDED =
  'This video is no longer being read here. Another window may already ask about it, or it could not be read, in which case the download list says why and offers Try again.';

/** How long a finished download's window stays before closing by itself. */
export const CLOSE_AFTER_MS = 5000;

export class DownloadWindowState {
  readonly #host: DownloadWindowHost;
  readonly #target: Target;
  /** A failed action says why in the window, until the next action starts. */
  readonly #runner = new Runner(
    (text) => (this.error = text),
    () => (this.error = null),
  );
  /** What the window does with the download it shows. */
  readonly progress = new ProgressActions({
    backend: () => this.#host.backend(),
    runner: this.#runner,
    downloadId: () => this.downloadId,
    row: () => this.row,
    pickFolder: (current) => this.changeFolder(current),
    close: () => this.close(),
  });
  /** Settings changes go to the app one after another, in the order they were made. */
  readonly #settingsWriter = new SettingsWriter((patch) =>
    this.#runner.succeeds(() => this.#host.backend().changeSettings(patch)),
  );

  phase = $state<Phase>('loading');
  /** The running app's name, for the window title: the backend's, the product name until it answers. */
  name = $state(PRODUCT_NAME);
  question = $state<Question | null>(null);
  row = $state<DownloadSummary | null>(null);
  queues = $state<Queue[]>([]);
  settings = $state<AppSettings | null>(null);
  /** The media add-on, once listed. */
  addon = $state<AddonInfo | null>(null);
  /** The download in the list this question is about again, if any. */
  earlier = $state<DownloadSummary | null>(null);
  /** Why the window has nothing more to show (`gone`). */
  goneReason = $state('');
  /** The last action that failed, in plain words; shown in the window. */
  error = $state<string | null>(null);
  busy = $state(false);
  /** A quality picked that needs the media add-on: it downloads once the add-on is in. */
  awaitingAddon = $state<{ selection: MediaSelection } | null>(null);
  /** The time its note on an earlier download says "today" against, as in the main window. */
  readonly clock = new Clock();

  #answered = false;
  #unsubscribe: (() => void) | null = null;
  /** Bumped by every init and dispose, so a superseded init lets go of what it set up. */
  #generation = 0;

  constructor(host: DownloadWindowHost, target: Target) {
    this.#target = target;
    this.#host = host;
  }

  /** The question's id, while the window was opened for one. */
  get questionId(): QuestionId | null {
    return 'question' in this.#target ? this.#target.question : null;
  }

  /** The download shown, once there is one. */
  get downloadId(): DownloadId | null {
    return this.row?.id ?? ('download' in this.#target ? this.#target.download : null);
  }

  queueName(id: Queue['id']): string {
    return this.queues.find((q) => q.id === id)?.name ?? 'Its queue';
  }

  /** Loads the window's question or download and starts listening. */
  async init(): Promise<void> {
    this.#teardown();
    const generation = ++this.#generation;
    const events = new HeldEvents((e) => this.handle(e));
    try {
      const unsubscribe = await this.#host.backend().subscribe(events.receive, () => {
        void this.init();
      });
      if (generation !== this.#generation) {
        // Closed (or loaded again) while subscribing: nothing would ever close this listener.
        unsubscribe();
        events.drop();
        return;
      }
      this.#unsubscribe = unsubscribe;
      this.clock.follow(this.#host.backend().sampleData?.now);
      this.clock.start();
      const [queues, settings, name] = await Promise.all([
        this.#host.backend().listQueues(),
        this.#host.backend().getSettings(),
        runningAppName(this.#host.backend()),
      ]);
      this.queues = queues;
      this.settings = settings;
      this.name = name;
      if ('download' in this.#target) {
        this.row = await this.#host.backend().getDownload(this.#target.download);
        this.phase = 'progress';
      } else {
        await this.#loadQuestion(this.#target.question);
      }
    } catch (err) {
      this.#gone(errorMessage(err));
    } finally {
      events.release();
    }
    void this.#loadAddon();
  }

  async #loadQuestion(id: QuestionId): Promise<void> {
    const question = await this.#host.backend().getQuestion(id);
    if (!question) {
      // Answered (here or anywhere) or taken back before the window loaded: never asked again.
      this.#gone(ANSWERED_ELSEWHERE);
      return;
    }
    this.question = question;
    this.phase = 'ask';
    try {
      this.earlier = await this.#host.backend().earlierDownload(id);
    } catch {
      // Only a note; the question works without it.
    }
  }

  async #loadAddon(): Promise<void> {
    try {
      const addons = await this.#host.backend().listAddons();
      this.addon = addons.find((a) => a.id === MEDIA_ADDON) ?? null;
    } catch {
      // The add-on events still bring it.
    }
  }

  dispose(): void {
    this.#generation += 1;
    this.#teardown();
  }

  #teardown(): void {
    this.clock.stop();
    this.#unsubscribe?.();
    this.#unsubscribe = null;
  }

  #gone(reason: string): void {
    this.phase = 'gone';
    this.goneReason = reason;
  }

  /** Whether this window is asking question `id` right now. */
  #asking(id: QuestionId): boolean {
    return id === this.questionId && this.phase === 'ask';
  }

  /** What this window keeps from each core event; most concern other windows. */
  readonly #handlers: UiEventHandlers = {
    downloads_changed: (e) => {
      const mine = this.downloadId;
      if (mine === null || this.phase !== 'progress') return;
      // A batch holds only the rows that changed: finding ours costs that, not the list.
      for (const row of e.data) if (row.id === mine) this.row = row;
    },
    downloads_removed: (e) => {
      const mine = this.downloadId;
      if (this.phase === 'progress' && mine !== null && e.data.includes(mine)) {
        this.#gone('This download was removed from the list.');
      }
    },
    queues_changed: (e) => {
      this.queues = e.data;
    },
    settings_changed: (e) => {
      this.settings = e.data;
    },
    addons_changed: (e) => this.#addonsChanged(e.data),
    media_analysing: (e) => {
      if (this.#asking(e.data.id) && this.question?.kind !== 'choice') {
        this.question = { kind: 'reading', data: e.data };
      }
    },
    media_choice_requested: (e) => {
      if (this.#asking(e.data.id)) this.#choiceArrived(e.data);
    },
    addon_required: (e) => {
      if (!this.#asking(e.data.id)) return;
      this.addon = e.data.addon;
      // A quality list stays: the quality picked waits for the add-on (see `choose`).
      if (this.question?.kind !== 'choice') {
        this.question = { kind: 'addon', data: { ...e.data } };
      }
    },
    confirmation_closed: (e) => {
      if (!this.#asking(e.data.id) || this.#answered) return;
      this.#gone(this.question?.kind === 'reading' ? READING_ENDED : ANSWERED_ELSEWHERE);
    },
    // About the main window and the download list, not one window's question.
    captured: ignore,
    confirmation_requested: ignore,
    settings_imported: ignore,
    storage_failed: ignore,
    window_requested: ignore,
  };

  /** Keeps what concerns this window from one core event. */
  handle(event: UiEvent): void {
    dispatch(this.#handlers, event);
  }

  /** The quality list for the video this window reads. */
  #choiceArrived(choice: MediaChoice): void {
    this.question = { kind: 'choice', data: choice };
    void this.#host
      .backend()
      .earlierDownload(choice.id)
      .then((row) => (this.earlier = row))
      .catch(() => {
        // Only a note, and the question works without it; but one read for the question
        // before may no longer hold, so none is shown.
        this.earlier = null;
      });
  }

  #addonsChanged(addons: AddonInfo[]): void {
    const media = addons.find((a) => a.id === MEDIA_ADDON) ?? null;
    this.addon = media;
    const waiting = this.awaitingAddon;
    if (waiting && isInstalled(media ?? undefined)) {
      this.awaitingAddon = null;
      void this.choose(waiting.selection);
    } else if (waiting && media?.state.type === 'not_installed') {
      // The install was stopped: nothing will come, so the quality list comes back.
      this.awaitingAddon = null;
    }
  }

  /** Download now (or later) for a browser file. */
  async confirm(answer: Answer, later: boolean): Promise<boolean> {
    const question = this.question;
    if (question?.kind !== 'capture' || this.busy) return false;
    const proposal = question.data;
    const confirmation: CaptureConfirmation = {
      file_name:
        answer.fileName?.trim() && answer.fileName.trim() !== proposal.file_name
          ? answer.fileName.trim()
          : null,
      folder: answer.folder && answer.folder !== proposal.folder ? answer.folder : null,
      queue: answer.queue !== null && answer.queue !== proposal.queue ? answer.queue : null,
      later,
    };
    this.busy = true;
    this.#answered = true;
    const id = await this.#runner.run(() =>
      this.#host.backend().confirmCapture(proposal.id, confirmation),
    );
    this.busy = false;
    if (id === undefined) {
      this.#answered = false;
      return false;
    }
    await this.#started(id, later);
    return true;
  }

  /**
   * Download now (or later) for a video. A quality whose video and audio must be joined while
   * the media add-on is missing installs the add-on first, then downloads by itself.
   */
  async choose(selection: MediaSelection): Promise<boolean> {
    const question = this.question;
    if (question?.kind !== 'choice' || this.busy) return false;
    const choice = question.data;
    const option = choice.options.find((o) => o.id === selection.option_id);
    const needsAddon = option?.needs_ffmpeg ?? false;
    if (needsAddon && this.#installFirst(selection)) return true;
    this.busy = true;
    this.#answered = true;
    // The core may ask for the add-on instead; the window offers it and goes on afterwards.
    const outcome = { addonAsked: false };
    const id = await this.#runner.run(
      () => this.#host.backend().chooseMedia(choice.id, selection),
      (err) => (outcome.addonAsked = needsAddon && isAddonRequired(err)),
    );
    this.busy = false;
    if (id === undefined) {
      this.#answered = false;
      if (outcome.addonAsked) this.awaitingAddon = { selection };
      return outcome.addonAsked;
    }
    await this.#started(id, selection.later);
    return true;
  }

  /**
   * A quality that needs the media add-on while it is missing: the quality waits, and the add-on
   * installs (unless it already is). False when the add-on is in, or not listed yet.
   */
  #installFirst(selection: MediaSelection): boolean {
    const addon = this.addon;
    if (!addon || isInstalled(addon)) return false;
    this.awaitingAddon = { selection };
    if (addon.state.type !== 'installing') void this.installAddon();
    return true;
  }

  /** The question was answered with download `id`: show its progress, or close. */
  async #started(id: DownloadId, later: boolean): Promise<void> {
    this.#host.becameDownload?.(id);
    if (later || this.settings?.show_progress_windows === false) {
      this.close();
      return;
    }
    this.question = null;
    this.earlier = null;
    this.phase = 'progress';
    try {
      this.row = await this.#host.backend().getDownload(id);
    } catch (err) {
      this.#gone(errorMessage(err));
    }
  }

  /** Cancel on a question: nothing is downloaded, and the window closes. */
  async discard(): Promise<void> {
    const question = this.question;
    const id = this.questionId;
    if (!question || id === null) {
      this.close();
      return;
    }
    this.#answered = true;
    const ok = await this.#runner.succeeds(() =>
      question.kind === 'capture'
        ? this.#host.backend().rejectCapture(id)
        : this.#host.backend().dismissMediaChoice(id),
    );
    if (ok) this.close();
    else this.#answered = false;
  }

  async installAddon(): Promise<void> {
    if (!this.addon) await this.#loadAddon();
    await this.#runner.run(() => this.#host.backend().installAddon(MEDIA_ADDON));
  }

  async stopAddonInstall(): Promise<void> {
    await this.#runner.run(() => this.#host.backend().cancelAddonOperation(MEDIA_ADDON));
  }

  /** Not now on the add-on: a picked quality gets its list back; a page waiting for it goes. */
  async declineAddon(): Promise<void> {
    if (this.awaitingAddon) {
      this.awaitingAddon = null;
      return;
    }
    await this.discard();
  }

  async changeFolder(current: string): Promise<string | null> {
    const picked = await this.#runner.run(() => this.#host.backend().pickFolder(current));
    return picked ?? null;
  }

  // ---- Settings shown in the window ----

  #saveSettings(change: (draft: AppSettings) => void): Promise<void> {
    return changeSettings(this, change, this.#settingsWriter);
  }

  /** "Start downloads without asking": the window before each browser file, inverted. */
  setStartWithoutAsking(on: boolean): Promise<void> {
    return this.#saveSettings((d) => void (d.extension.ask_before_download = !on));
  }

  /** "Close this window when downloads start". */
  setCloseWhenStarted(on: boolean): Promise<void> {
    return this.#saveSettings((d) => void (d.show_progress_windows = !on));
  }

  close(): void {
    this.#host.close();
  }
}
