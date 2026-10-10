// The one boundary between the interface and whatever runs the downloads.
// `TauriBackend` talks to the Rust side; `MockBackend` simulates it in memory.

import type {
  AddonInfo,
  AddRequest,
  AppSettings,
  CaptureConfirmation,
  DownloadId,
  DownloadSummary,
  MediaSelection,
  PendingCapture,
  Queue,
  QueueId,
  UiEvent,
} from '@wirecatch/protocol';

export type Unsubscribe = () => void;

/**
 * Signals from the desktop shell that are not core events. `resync`: some events were missed,
 * so everything must be loaded again. `links`: links arrived from the command line (a second
 * launch, `wirecatch-app https://...`); fetch them with `takeLaunchLinks`.
 */
export type ShellSignal = 'resync' | 'links';

/** Only the mock backend has this: palette actions that pretend the browser sent a download. */
export interface SampleData {
  commands: ReadonlyArray<{ id: string; title: string; keywords: string; run: () => void }>;
  /** The sample clock, so schedule labels agree with the simulation. */
  now: () => number;
}

export interface Backend {
  /** Which implementation this is, so the interface can label sample data. */
  readonly kind: 'tauri' | 'mock';
  /** Present only on the mock backend. */
  readonly sampleData?: SampleData;

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
  updateSettings(settings: AppSettings): Promise<void>;
  exportSettings(path: string): Promise<void>;
  importSettings(path: string): Promise<void>;

  pendingConfirmations(): Promise<PendingCapture[]>;
  confirmCapture(id: number, confirmation: CaptureConfirmation): Promise<void>;
  rejectCapture(id: number): Promise<void>;

  openFile(id: DownloadId): Promise<void>;
  openFolder(id: DownloadId): Promise<void>;
  /** Moves a download, running or not, to another folder. */
  setDownloadFolder(id: DownloadId, folder: string): Promise<void>;

  /** Links passed on the command line that the interface has not shown yet, handed over once. */
  takeLaunchLinks(): Promise<string[]>;

  /** Downloads the chosen quality of a media choice. */
  chooseMedia(id: number, selection: MediaSelection): Promise<void>;
  /** Drops a media choice the person did not want. */
  dismissMediaChoice(id: number): Promise<void>;

  /** Optional components (D2, D20): one media add-on with yt-dlp, ffmpeg and Deno. */
  listAddons(): Promise<AddonInfo[]>;
  installAddon(id: string): Promise<void>;
  updateAddon(id: string): Promise<void>;
  removeAddon(id: string): Promise<void>;

  /** Every change the core pushes, in order. */
  subscribe(listener: (event: UiEvent) => void): Promise<Unsubscribe>;
  /** Signals from the desktop shell (see `ShellSignal`). */
  onShellSignal(listener: (signal: ShellSignal) => void): Promise<Unsubscribe>;

  /** System pickers. They resolve to `null` when the person cancels. */
  pickFolder(current?: string): Promise<string | null>;
  pickSaveFile(suggestedName: string): Promise<string | null>;
  pickOpenFile(): Promise<string | null>;
}

/**
 * Tauri command names, exactly as the core registers them. The Rust side in `src-tauri`
 * registers the same names; a test checks both lists agree.
 */
export const COMMANDS = {
  listDownloads: 'list_downloads',
  addDownload: 'add_download',
  pause: 'pause',
  resume: 'resume',
  cancel: 'cancel',
  remove: 'remove',
  retry: 'retry',
  refreshLink: 'refresh_link',
  moveToQueue: 'move_to_queue',
  reorder: 'reorder',
  pauseAll: 'pause_all',
  resumeAll: 'resume_all',
  listQueues: 'list_queues',
  createQueue: 'create_queue',
  updateQueue: 'update_queue',
  deleteQueue: 'delete_queue',
  setQueuePaused: 'set_queue_paused',
  startQueueNow: 'start_queue_now',
  getSettings: 'get_settings',
  updateSettings: 'update_settings',
  exportSettings: 'export_settings',
  importSettings: 'import_settings',
  pendingConfirmations: 'pending_confirmations',
  confirmCapture: 'confirm_capture',
  rejectCapture: 'reject_capture',
  openFile: 'open_file',
  openFolder: 'open_folder',
  setDownloadFolder: 'set_download_folder',
  takeLaunchLinks: 'take_launch_links',
  chooseMedia: 'choose_media',
  dismissMediaChoice: 'dismiss_media_choice',
  listAddons: 'list_addons',
  installAddon: 'install_addon',
  updateAddon: 'update_addon',
  removeAddon: 'remove_addon',
} as const;

/** The single Tauri event that carries every `UiEvent`. */
export const EVENT_NAME = 'wirecatch://event';

/** Shell signals, as Tauri events with no payload. */
export const SIGNAL_EVENTS: Record<ShellSignal, string> = {
  resync: 'wirecatch://resync',
  links: 'wirecatch://links',
};

/** An error from the backend, with a message written for a person. */
export class BackendError extends Error {
  override name = 'BackendError';
}
