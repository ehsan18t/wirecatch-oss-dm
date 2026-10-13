// Generated from crates/protocol by `pnpm protocol:generate`. Do not edit; run it again after changing the constants there.

import type { ExtensionSettings } from './ExtensionSettings';
import type { OutputFormat } from './OutputFormat';
import type { QueueId } from './QueueId';

/**
 * Bumped on any incompatible change to the wire format.
 */
export const PROTOCOL_VERSION = 1;

/**
 * The default queue, which always exists and cannot be deleted: downloads go to it when
 * nothing names another queue.
 */
export const DEFAULT_QUEUE_ID: QueueId = 1;

/**
 * Request id the app uses for an error it cannot tie to a request (for example an
 * unreadable frame). Clients start their ids at 1.
 */
export const UNKNOWN_REQUEST_ID = 0;

/**
 * Largest message body in bytes. Browsers refuse messages from a native host larger
 * than 1 MiB; the same cap applies to every local connection so no side can be made to
 * allocate more.
 */
export const MAX_FRAME_LEN = 1048576;

/**
 * Highest accepted [`AppSettings::connections_per_download`].
 */
export const MAX_CONNECTIONS_PER_DOWNLOAD = 32;

/**
 * [`AppSettings::connections_per_download`] on a fresh install.
 */
export const DEFAULT_CONNECTIONS_PER_DOWNLOAD = 8;

/**
 * Highest accepted value for any limit on downloads running at once: the app-wide
 * [`AppSettings::max_concurrent_downloads`] and each [`Queue::max_concurrent`].
 */
export const MAX_CONCURRENT_DOWNLOADS = 100;

/**
 * [`AppSettings::max_concurrent_downloads`] on a fresh install.
 */
export const DEFAULT_MAX_CONCURRENT_DOWNLOADS = 5;

/**
 * [`Queue::max_concurrent`] of a newly created queue.
 */
export const NEW_QUEUE_MAX_CONCURRENT = 2;

/**
 * [`Queue::max_concurrent`] of the default queue on a fresh install.
 */
export const DEFAULT_QUEUE_MAX_CONCURRENT = 3;

/**
 * [`ProcessingProgress::per_mille`] of a finished step: progress is counted in tenths of
 * a percent.
 */
export const PER_MILLE_DONE = 1000;

/**
 * [`SettingsProfile::format_version`] of the settings files this build writes, and the
 * newest it reads.
 */
export const PROFILE_FORMAT_VERSION = 1;

/**
 * Most download windows asking a question at once (D23); later questions wait until one
 * of them closes.
 */
export const MAX_QUESTION_WINDOWS = 5;

/**
 * How far each download window sits down and right of the one before it, in logical
 * pixels.
 */
export const DOWNLOAD_WINDOW_CASCADE_STEP = 28;

/**
 * The Tauri event that carries every [`UiEvent`](crate::app::UiEvent), in order.
 */
export const UI_EVENT = 'wirecatch://event';

/**
 * The Tauri event, with no payload, that says events were missed: reload everything.
 */
export const RESYNC_EVENT = 'wirecatch://resync';

/**
 * The Tauri event, with no payload, that says links arrived from a second launch: fetch
 * them with `take_launch_links`.
 */
export const LINKS_EVENT = 'wirecatch://links';

/**
 * The page a download window shows, relative to the app's address.
 */
export const DOWNLOAD_WINDOW_PAGE = 'window/download';

/**
 * The query parameter of [`DOWNLOAD_WINDOW_PAGE`] naming the question a window asks.
 */
export const QUESTION_PARAMETER = 'question';

/**
 * The query parameter of [`DOWNLOAD_WINDOW_PAGE`] naming the download a window shows.
 */
export const DOWNLOAD_PARAMETER = 'download';

/**
 * File types that open directly, without asking first: common video, audio, image, document
 * and archive types that do not run code when opened. Every other type, and a file with no
 * extension, could run code (programs, installers, scripts, shortcuts, disk images, Office
 * files with macros and many more), so opening it needs the person's confirmation. An
 * allowlist fails safe: a type nobody thought of asks first.
 *
 * In order: video, audio, subtitles, images, documents, archives. Left out on purpose among
 * documents: Office files with macros (`docm` and friends), `rtf` (embedded objects), the
 * OpenDocument formats (macros) and `csv` (formulas that run when a spreadsheet opens it).
 * `docx`, `xlsx` and `pptx` stay: they cannot hold macros, and Office opens downloaded ones,
 * which carry the mark of the web, in Protected View.
 *
 * The shell decides with it (`is_program` in `apps/desktop/src-tauri/src/api.rs`); the interface
 * reads it for wording and the sample backend.
 */
export const SAFE_EXTENSIONS: readonly string[] = [
  'mp4',
  'm4v',
  'mkv',
  'webm',
  'mov',
  'avi',
  'wmv',
  'flv',
  'mpg',
  'mpeg',
  'ts',
  'm2ts',
  '3gp',
  'ogv',
  'mp3',
  'm4a',
  'aac',
  'flac',
  'wav',
  'ogg',
  'oga',
  'opus',
  'wma',
  'aiff',
  'mka',
  'srt',
  'vtt',
  'jpg',
  'jpeg',
  'png',
  'gif',
  'webp',
  'bmp',
  'tif',
  'tiff',
  'heic',
  'heif',
  'avif',
  'pdf',
  'txt',
  'md',
  'docx',
  'xlsx',
  'pptx',
  'epub',
  'zip',
  '7z',
  'rar',
  'tar',
  'gz',
  'tgz',
  'bz2',
  'xz',
  'zst',
];

/**
 * The audio bit rates, in kbps, a person can pick when saving as each format;
 * formats without a choice are left out (`OutputFormat::kbps_choices`).
 */
export const AUDIO_KBPS_CHOICES: Readonly<Partial<Record<OutputFormat, readonly number[]>>> = {
  mp3: [128, 192, 256, 320],
  m4a: [128, 192, 256],
  opus: [96, 128, 160, 192],
};

/**
 * The audio bit rate, in kbps, used when the audio of a download saved as each
 * format must be re-encoded (`OutputFormat::default_kbps`).
 */
export const AUDIO_DEFAULT_KBPS: Readonly<Partial<Record<OutputFormat, number>>> = {
  mp3: 192,
  m4a: 192,
  opus: 128,
};

/**
 * The extension's settings on a fresh install (`ExtensionSettings::default`), which
 * the extension judges browser downloads by until it first hears from the app.
 */
export const DEFAULT_EXTENSION_SETTINGS: ExtensionSettings = {
  capture_enabled: true,
  ask_before_download: true,
  show_media_button: true,
  ignored_sites: [],
  capture_extensions: [],
  min_capture_size: 0,
  default_queue: 1,
};
