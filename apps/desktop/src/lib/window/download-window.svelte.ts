// Everything one download window knows and does (D23). A window shows one question (confirm a
// browser file, a video being read, pick a quality, install the media add-on) and, after
// Download now, that download's progress. It loads only its own question or download, and of
// each event keeps only what concerns it, so a window costs nothing per download in the list.

import type {
  AddonInfo,
  AppSettings,
  CaptureConfirmation,
  DownloadId,
  DownloadSummary,
  FixAction,
  MediaSelection,
  Queue,
  UiEvent,
} from '@wirecatch/protocol';
import { isAddonRequired, isInstalled, MEDIA_ADDON } from '../addons';
import type { Backend, Question } from '../backend/types';

/** What the window is showing. */
export type Phase = 'loading' | 'ask' | 'progress' | 'gone';

/** What the window needs from whatever shows it: a real window, or the sample page. */
export interface WindowHost {
  /** Closes the window. Never answers its question. */
  close(): void;
  /** The window now shows download `id` (only the sample page needs telling). */
  becameDownload?(id: DownloadId): void;
}

/** What the window was opened for. */
export type Target = { question: number } | { download: DownloadId };

/** The answer in a question window's form. */
export interface Answer {
  fileName: string | null;
  folder: string | null;
  queue: Queue['id'] | null;
}

function message(err: unknown): string {
  if (err instanceof Error && err.message) return err.message;
  if (typeof err === 'string' && err) return err;
  return 'Something went wrong. Try again.';
}

/** Said when a question closes without an answer from this window. */
export const ANSWERED_ELSEWHERE =
  'This download is no longer waiting. It was answered in another window, or the browser took it back.';

/** Said when a video being read closes without a quality list. */
export const READING_ENDED =
  'This video is no longer being read here. Another window may already ask about it, or it could not be read, in which case the download list says why and offers Try again.';

export class DownloadWindowState {
  readonly backend: Backend;
  readonly #host: WindowHost;
  readonly #target: Target;

  phase = $state<Phase>('loading');
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
  // Replaced whole, never mutated, so a plain Date is enough.
  // eslint-disable-next-line svelte/prefer-svelte-reactivity
  now = $state(new Date());

  #answered = false;
  #unsubscribe: (() => void) | null = null;

  constructor(backend: Backend, target: Target, host: WindowHost) {
    this.backend = backend;
    this.#target = target;
    this.#host = host;
  }

  /** The question's id, while the window was opened for one. */
  get questionId(): number | null {
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
    const buffered: UiEvent[] = [];
    let loading = true;
    try {
      this.#unsubscribe = await this.backend.subscribe((e) => {
        if (loading) buffered.push(e);
        else this.handle(e);
      });
      const [queues, settings] = await Promise.all([
        this.backend.listQueues(),
        this.backend.getSettings(),
      ]);
      this.queues = queues;
      this.settings = settings;
      if ('download' in this.#target) {
        this.row = await this.backend.getDownload(this.#target.download);
        this.phase = 'progress';
      } else {
        await this.#loadQuestion(this.#target.question);
      }
    } catch (err) {
      this.#gone(message(err));
    } finally {
      loading = false;
      for (const e of buffered) this.handle(e);
    }
    void this.#loadAddon();
  }

  async #loadQuestion(id: number): Promise<void> {
    const question = await this.backend.getQuestion(id);
    if (!question) {
      // Answered (here or anywhere) or taken back before the window loaded: never asked again.
      this.#gone(ANSWERED_ELSEWHERE);
      return;
    }
    this.question = question;
    this.phase = 'ask';
    try {
      this.earlier = await this.backend.earlierDownload(id);
    } catch {
      // Only a note; the question works without it.
    }
  }

  async #loadAddon(): Promise<void> {
    try {
      const addons = await this.backend.listAddons();
      this.addon = addons.find((a) => a.id === MEDIA_ADDON) ?? null;
    } catch {
      // The add-on events still bring it.
    }
  }

  dispose(): void {
    this.#unsubscribe?.();
    this.#unsubscribe = null;
  }

  #gone(reason: string): void {
    this.phase = 'gone';
    this.goneReason = reason;
  }

  /** Keeps what concerns this window from one core event. */
  handle(event: UiEvent): void {
    const id = this.questionId;
    switch (event.type) {
      case 'downloads_changed': {
        const mine = this.downloadId;
        if (mine === null || this.phase !== 'progress') return;
        // A batch holds only the rows that changed: finding ours costs that, not the list.
        for (const row of event.data) if (row.id === mine) this.row = row;
        return;
      }
      case 'downloads_removed':
        if (this.phase === 'progress' && this.downloadId !== null) {
          if (event.data.includes(this.downloadId)) {
            this.#gone('This download was removed from the list.');
          }
        }
        return;
      case 'queues_changed':
        this.queues = event.data;
        return;
      case 'settings_changed':
        this.settings = event.data;
        return;
      case 'addons_changed':
        this.#addonsChanged(event.data);
        return;
      case 'media_analysing':
        if (event.data.id === id && this.phase === 'ask' && this.question?.kind !== 'choice') {
          this.question = { kind: 'reading', data: event.data };
        }
        return;
      case 'media_choice_requested':
        if (event.data.id === id && this.phase === 'ask') {
          this.question = { kind: 'choice', data: event.data };
          void this.backend
            .earlierDownload(event.data.id)
            .then((row) => (this.earlier = row))
            .catch(() => {});
        }
        return;
      case 'addon_required':
        if (event.data.id !== id || this.phase !== 'ask') return;
        this.addon = event.data.addon;
        // A quality list stays: the quality picked waits for the add-on (see `choose`).
        if (this.question?.kind !== 'choice') {
          this.question = { kind: 'addon', data: { ...event.data } };
        }
        return;
      case 'confirmation_closed':
        if (event.data.id !== id || this.#answered || this.phase !== 'ask') return;
        this.#gone(this.question?.kind === 'reading' ? READING_ENDED : ANSWERED_ELSEWHERE);
        return;
      default:
        return;
    }
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

  async #act<T>(action: () => Promise<T>): Promise<T | undefined> {
    this.error = null;
    try {
      return await action();
    } catch (err) {
      this.error = message(err);
      return undefined;
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
    const id = await this.#act(() => this.backend.confirmCapture(proposal.id, confirmation));
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
    if (needsAddon && this.addon && !isInstalled(this.addon)) {
      this.awaitingAddon = { selection };
      if (this.addon.state.type !== 'installing') void this.installAddon();
      return true;
    }
    this.busy = true;
    this.#answered = true;
    this.error = null;
    let id: DownloadId;
    try {
      id = await this.backend.chooseMedia(choice.id, selection);
    } catch (err) {
      this.#answered = false;
      this.busy = false;
      if (needsAddon && isAddonRequired(message(err))) {
        // The core asked for the add-on instead; the window offers it and goes on afterwards.
        this.awaitingAddon = { selection };
        return true;
      }
      this.error = message(err);
      return false;
    }
    this.busy = false;
    await this.#started(id, selection.later);
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
      this.row = await this.backend.getDownload(id);
    } catch (err) {
      this.#gone(message(err));
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
    const ok = await this.#act(async () => {
      if (question.kind === 'capture') await this.backend.rejectCapture(id);
      else await this.backend.dismissMediaChoice(id);
      return true;
    });
    if (ok) this.close();
    else this.#answered = false;
  }

  async installAddon(): Promise<void> {
    if (!this.addon) await this.#loadAddon();
    await this.#act(() => this.backend.installAddon(MEDIA_ADDON));
  }

  async stopAddonInstall(): Promise<void> {
    await this.#act(() => this.backend.cancelAddonOperation(MEDIA_ADDON));
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
    const picked = await this.#act(() => this.backend.pickFolder(current));
    return picked ?? null;
  }

  // ---- Progress ----

  async #onRow(action: (id: DownloadId) => Promise<void>): Promise<boolean> {
    const id = this.downloadId;
    if (id === null) return false;
    const ok = await this.#act(async () => {
      await action(id);
      return true;
    });
    return ok === true;
  }

  pause(): Promise<boolean> {
    return this.#onRow((id) => this.backend.pause(id));
  }

  resume(): Promise<boolean> {
    // A cancelled download has no partial file left; it starts over.
    return this.#onRow((id) =>
      this.row?.status === 'cancelled' ? this.backend.retry(id) : this.backend.resume(id),
    );
  }

  retry(): Promise<boolean> {
    return this.#onRow((id) => this.backend.retry(id));
  }

  /** Cancel on a download: stops it, deletes the partial file and takes it off the list. */
  async cancelDownload(): Promise<void> {
    if (await this.#onRow((id) => this.backend.remove([id], true))) this.close();
  }

  /** Whether opening the finished file could run code, so the window asks first. */
  async isProgram(): Promise<boolean | undefined> {
    const id = this.downloadId;
    if (id === null) return undefined;
    return this.#act(() => this.backend.isProgram(id));
  }

  openFile(allowProgram = false): Promise<boolean> {
    return this.#onRow((id) => this.backend.openFile(id, allowProgram));
  }

  openFolder(): Promise<boolean> {
    return this.#onRow((id) => this.backend.openFolder(id));
  }

  refreshLink(url: string): Promise<boolean> {
    return this.#onRow((id) => this.backend.refreshLink(id, url));
  }

  /** Runs the fix an error offers. A fresh link needs the link: `askLink` shows that form. */
  async fix(action: FixAction, askLink: () => void): Promise<void> {
    switch (action) {
      case 'retry':
        await this.retry();
        break;
      case 'refresh_link':
        askLink();
        break;
      case 'choose_folder': {
        const row = this.row;
        if (!row) return;
        const picked = await this.changeFolder(row.path);
        if (!picked) return;
        if (await this.#onRow((id) => this.backend.setDownloadFolder(id, picked))) {
          await this.retry();
        }
        break;
      }
      case 'free_disk_space':
        await this.openFolder();
        break;
    }
  }

  // ---- Settings shown in the window ----

  async #saveSettings(change: (draft: AppSettings) => void): Promise<void> {
    if (!this.settings) return;
    const before = $state.snapshot(this.settings);
    const draft = $state.snapshot(this.settings);
    change(draft);
    this.settings = draft;
    const ok = await this.#act(async () => {
      await this.backend.updateSettings(draft);
      return true;
    });
    if (!ok) this.settings = before;
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
