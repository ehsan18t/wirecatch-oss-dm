// The one boundary between the interface and whatever runs the downloads.
// `TauriBackend` talks to the Rust side; `MockBackend` simulates it in memory.

import type {
  AddonId,
  AddonInfo,
  AddonRequestItem,
  AddRequest,
  AppSettings,
  AppSettingsPatch,
  CaptureConfirmation,
  DownloadId,
  DownloadSummary,
  ErrorCode,
  ImportPreview,
  MediaChoice,
  MediaSelection,
  PendingCapture,
  Question,
  QuestionId,
  Queue,
  QueueId,
  SuffixListInfo,
  UiEvent,
} from '@wirecatch/protocol';
import type { WindowRegistry } from '../sample-windows';

export type Unsubscribe = () => void;

/** A capture that needs an add-on, as `UiEvent::AddonRequired` carries it. */
export type AddonRequest = Omit<AddonRequestItem, 'id'>;

/**
 * Signals from the desktop shell that are not core events. `resync`: some events were missed,
 * so everything must be loaded again. `links`: links arrived from the command line (a second
 * launch, `wirecatch-app https://...`); fetch them with `takeLaunchLinks`.
 */
export type ShellSignal = 'resync' | 'links';

/** Only the mock backend has this: palette actions that pretend the browser sent a download. */
export interface SampleData {
  commands: readonly { id: string; title: string; keywords: string; run: () => void }[];
  /** The sample clock, so schedule labels agree with the simulation. */
  now: () => number;
  /** The download windows, which the sample page shows inside itself. */
  windows: WindowRegistry;
}

export interface Backend {
  /** Which implementation this is, so the interface can label sample data. */
  readonly kind: 'tauri' | 'mock';
  /** Present only on the mock backend. */
  readonly sampleData?: SampleData;
  /** The name the running app shows: its channel's, so a debug build is "Wirecatch Dev". */
  appName(): Promise<string>;

  listDownloads(): Promise<DownloadSummary[]>;
  addDownload(request: AddRequest): Promise<DownloadId>;
  pause(id: DownloadId): Promise<void>;
  resume(id: DownloadId): Promise<void>;
  /** Stops the download and discards the partial file. The row stays so it can start again. */
  cancel(id: DownloadId): Promise<void>;
  remove(ids: DownloadId[], deleteFile: boolean): Promise<void>;
  retry(id: DownloadId): Promise<void>;
  /** Swaps in a fresh link for an expired one, keeping the bytes already downloaded. */
  refreshLink(id: DownloadId, url: string): Promise<void>;
  /** Saves a video whose conversion failed as it was downloaded instead (D24). */
  keepOriginal(id: DownloadId): Promise<void>;
  moveToQueue(ids: DownloadId[], queue: QueueId): Promise<void>;
  reorder(queue: QueueId, idsInOrder: DownloadId[]): Promise<void>;
  pauseAll(): Promise<void>;
  resumeAll(): Promise<void>;

  listQueues(): Promise<Queue[]>;
  createQueue(name: string): Promise<Queue>;
  updateQueue(queue: Queue): Promise<void>;
  deleteQueue(id: QueueId): Promise<void>;
  /** A paused queue starts nothing new and pauses what it was running. */
  setQueuePaused(id: QueueId, paused: boolean): Promise<void>;
  /** Runs a scheduled queue now, outside its hours, until it is empty or paused. */
  startQueueNow(id: QueueId): Promise<void>;

  getSettings(): Promise<AppSettings>;
  /** Changes only the settings in the patch, applied to the app's current settings. */
  changeSettings(patch: AppSettingsPatch): Promise<void>;
  exportSettings(path: string): Promise<void>;
  /** What importing the settings file at `path` would change, without changing anything. */
  previewImport(path: string): Promise<ImportPreview>;
  importSettings(path: string): Promise<void>;

  pendingConfirmations(): Promise<PendingCapture[]>;
  /** Starts (or with `later`, adds paused) a confirmed capture; the new download's id. */
  confirmCapture(id: QuestionId, confirmation: CaptureConfirmation): Promise<DownloadId>;
  rejectCapture(id: QuestionId): Promise<void>;

  /** The open question `id`, or `null` once it is answered, dismissed or taken back. */
  getQuestion(id: QuestionId): Promise<Question | null>;
  /** One download, for its progress window. */
  getDownload(id: DownloadId): Promise<DownloadSummary>;
  /** The download in the list that question `id` is about again, if any. */
  earlierDownload(id: QuestionId): Promise<DownloadSummary | null>;
  /** Brings every open question's window to the front, opening closed ones again. */
  showQuestionWindows(): Promise<void>;
  /** Opens (or brings forward) the progress window of a download. */
  showProgressWindow(id: DownloadId): Promise<void>;
  /** Tells the shell the main window has drawn its first view, so it can be shown without a blank flash. */
  mainWindowReady(): Promise<void>;

  /**
   * Opens a finished file. A program (an installer, a script, a shortcut) opens only once the
   * person agreed to the shell's own warning, which no page can answer; declining is no error.
   */
  openFile(id: DownloadId): Promise<void>;
  openFolder(id: DownloadId): Promise<void>;
  /** Moves a download, running or not, to another folder. */
  setDownloadFolder(id: DownloadId, folder: string): Promise<void>;

  /** Links passed on the command line that the interface has not shown yet, handed over once. */
  takeLaunchLinks(): Promise<string[]>;

  /** Downloads (or with `later`, adds paused) the chosen quality; the new download's id. */
  chooseMedia(id: QuestionId, selection: MediaSelection): Promise<DownloadId>;
  /** Drops a media choice the person did not want. */
  dismissMediaChoice(id: QuestionId): Promise<void>;
  /** Media choices still open, so a new or reloaded window asks them again. */
  pendingMediaChoices(): Promise<MediaChoice[]>;
  /** Pages waiting for an add-on, oldest first, so a new or reloaded window offers it again. */
  pendingAddonRequests(): Promise<AddonRequestItem[]>;

  /** Optional components (D2, D20): one media add-on with yt-dlp, ffmpeg and Deno. */
  listAddons(): Promise<AddonInfo[]>;
  installAddon(id: AddonId): Promise<void>;
  updateAddon(id: AddonId): Promise<void>;
  removeAddon(id: AddonId): Promise<void>;
  /** Stops an install or update; an update keeps the installed version working. */
  cancelAddonOperation(id: AddonId): Promise<void>;
  /** Looks for a newer media add-on now; the add-ons as they stand afterwards. */
  checkAddonUpdates(): Promise<AddonInfo[]>;

  /** The Public Suffix List in use and when a newer one was last looked for (D44). */
  suffixList(): Promise<SuffixListInfo>;
  /** Looks for a newer Public Suffix List now; the list in use afterwards. */
  checkSuffixList(): Promise<SuffixListInfo>;

  /** Every change the core pushes, in order; `reload` when one could not be read, so the caller loads what it shows again. */
  subscribe(listener: (event: UiEvent) => void, reload: () => void): Promise<Unsubscribe>;
  /** Signals from the desktop shell (see `ShellSignal`). */
  onShellSignal(listener: (signal: ShellSignal) => void): Promise<Unsubscribe>;

  /** System pickers. They resolve to `null` when the person cancels. */
  pickFolder(current?: string): Promise<string | null>;
  pickSaveFile(suggestedName: string): Promise<string | null>;
  pickOpenFile(): Promise<string | null>;
}

/** The backend as a download window's own webview sees it: it can also size and close itself. */
export interface DownloadWindowBackend extends Backend {
  /** Closes the download window this page runs in. */
  closeDownloadWindow(): Promise<void>;
  /** Makes the download window this page runs in as tall as its content (logical pixels). */
  fitDownloadWindow(height: number): Promise<void>;
}

/**
 * An error from the backend, with a message written for a person and, when the core or the shell
 * gave one, a code to act on. Decide from `code`, never from the wording.
 */
export class BackendError extends Error {
  override name = 'BackendError';
  readonly code: ErrorCode | null;

  constructor(message: string, code: ErrorCode | null = null) {
    super(message);
    this.code = code;
  }
}
