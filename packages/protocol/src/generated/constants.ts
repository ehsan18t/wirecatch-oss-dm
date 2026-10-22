// Generated from crates/protocol by `pnpm protocol:generate`. Do not edit; run it again after changing the constants there.

import type { Accent } from './Accent';
import type { Call } from './Call';
import type { ErrorCode } from './ErrorCode';
import type { Event } from './Event';
import type { ExtensionSettings } from './ExtensionSettings';
import type { FixAction } from './FixAction';
import type { OutputFormat } from './OutputFormat';
import type { QueueId } from './QueueId';
import type { Theme } from './Theme';

/**
 * The newest protocol version this build speaks. Raised by every change to the wire format
 * that a peer may need to know about, such as a new call, event, error code or field; a
 * change only this build can see, such as a clearer error message, leaves it (D32).
 */
export const PROTOCOL_VERSION = 2;

/**
 * The oldest protocol version this build still speaks. Raised only by a change an older
 * peer cannot read, which then cannot connect at all and is told to update (D32).
 */
export const MIN_PROTOCOL_VERSION = 1;

/**
 * The default queue, which always exists and cannot be deleted: downloads go to it when
 * nothing names another queue.
 */
export const DEFAULT_QUEUE_ID = 1 as QueueId;

/**
 * Request id the app uses for an error it cannot tie to a request (for example an
 * unreadable frame). Clients start their ids at 1.
 */
export const UNKNOWN_REQUEST_ID = 0;

/**
 * How long the bridge waits for the app, in milliseconds, from its first connection
 * attempt to giving up, starting the app included. A client that may make the bridge
 * start the app waits longer than this for the answer to `hello`.
 */
export const BRIDGE_START_BUDGET_MS = 5000;

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
 * A download window's width, in logical pixels; the sample app's windows take it too, so
 * they lay out as the real ones do.
 */
export const DOWNLOAD_WINDOW_WIDTH = 460;

/**
 * The least height a download window keeps, in logical pixels, however little it shows.
 */
export const DOWNLOAD_WINDOW_MIN_HEIGHT = 320;

/**
 * The most height a download window takes, in logical pixels, however much it shows: past
 * it the window's body scrolls, so opening a folded group never makes a towering window.
 */
export const DOWNLOAD_WINDOW_MAX_HEIGHT = 680;

/**
 * Every [`Theme`], in the order settings offer them.
 */
export const THEMES: readonly Theme[] = ['system', 'light', 'dark'];

/**
 * The fixes that trying the download again can carry out, so a notification and the
 * download complete window offer Try again for them (D52). The others need the person's
 * answer first: a fresh link, another folder, or keeping the original.
 */
export const RETRY_FIXES: readonly FixAction[] = ['retry', 'free_disk_space'];

/**
 * Every [`Accent`], in the order settings offer them. The design system in `packages/ui`
 * reads this list and defines a palette for each of these but `system`; its tests fail on
 * an accent without one.
 */
export const ACCENTS: readonly Accent[] = [
  'cyan',
  'purple',
  'blue',
  'green',
  'orange',
  'pink',
  'red',
  'teal',
  'indigo',
  'amber',
  'system',
];

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
 * The Tauri event, sent to one window, that carries its [`WindowFrame`] after it changed.
 */
export const WINDOW_FRAME_EVENT = 'wirecatch://window-frame';

/**
 * The Tauri event, sent to every window, that carries the [`SystemAppearance`] after the
 * system changed it.
 */
export const SYSTEM_APPEARANCE_EVENT = 'wirecatch://system-appearance';

/**
 * The Tauri event, sent to the main window, that carries a [`Reveal`]: the person clicked a
 * notification about these downloads (D52).
 */
export const REVEAL_EVENT = 'wirecatch://reveal';

/**
 * The Tauri event, sent to the download complete window, that carries its [`FinishedList`]
 * after it changed (D52).
 */
export const FINISHED_EVENT = 'wirecatch://finished';

/**
 * The most downloads one [`FinishedGroup`] lists; a queue's summary counts the rest.
 */
export const MAX_FINISHED_LISTED = 100;

/**
 * The most groups the download complete window keeps; the oldest go first.
 */
export const MAX_FINISHED_GROUPS = 20;

/**
 * The page a download window shows, relative to the app's address.
 */
export const DOWNLOAD_WINDOW_PAGE = 'download.html';

/**
 * The query parameter of [`DOWNLOAD_WINDOW_PAGE`] naming the question a window asks.
 */
export const QUESTION_PARAMETER = 'question';

/**
 * The query parameter of [`DOWNLOAD_WINDOW_PAGE`] naming the download a window shows.
 */
export const DOWNLOAD_PARAMETER = 'download';

/**
 * The query parameter of [`DOWNLOAD_WINDOW_PAGE`] that makes it the download complete
 * window (D52).
 */
export const FINISHED_PARAMETER = 'finished';

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
 * The shell decides with it (`is_program` in `apps/desktop/src-tauri/src/api.rs`) and asks in
 * a warning of its own; the sample backend reads it to ask the same way.
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
 * Most sites the list of sites to ignore holds. A patch naming more than this in one of its lists is refused before any site in it is read (D37.1).
 */
export const MAX_IGNORED_SITES = 1000;

/**
 * Most sites the person can add to the video sites the apps know (D51).
 */
export const MAX_VIDEO_SITES = 200;

/**
 * Longest site in the list of sites to ignore, in characters: the longest host name DNS allows.
 */
export const MAX_SITE_CHARS = 253;

/**
 * Words that make a name a secret's when one of its segments is the word (see the module documentation for how a name is split). `key` alone counts, so `Invalid key: 5` loses its 5: a bare `key=` pair is most often an API key.
 */
export const SECRET_WORDS: readonly string[] = [
  'token',
  'secret',
  'password',
  'passwd',
  'passphrase',
  'pass',
  'pwd',
  'passcode',
  'session',
  'sessionid',
  'sessid',
  'jsessionid',
  'phpsessid',
  'sid',
  'sig',
  'signature',
  'hdnts',
  'key',
  'apikey',
  'accesskey',
  'privatekey',
  'private',
  'auth',
  'authentication',
  'authorization',
  'bearer',
  'credential',
  'credentials',
  'cookie',
  'cookies',
  'csrf',
  'xsrf',
  'jwt',
  'otp',
];

/**
 * Endings that make a segment run together from words a secret's: `accesstoken`, `clientsecret`, `oauth`, `proxyauthorization`, `xapikey`. Only as an ending, so `tokens` and `author` keep their values.
 */
export const SECRET_ENDINGS: readonly string[] = [
  'token',
  'secret',
  'password',
  'passwd',
  'passphrase',
  'passcode',
  'auth',
  'authorization',
  'credential',
  'credentials',
  'cookie',
  'apikey',
  'accesskey',
  'privatekey',
  'sessionid',
  'sessid',
  'signature',
];

/**
 * Secret words that name a password, whose value after a `:` may be several words (`password: correct horse battery staple`).
 */
export const PASSWORD_WORDS: readonly string[] = [
  'password',
  'passwd',
  'passphrase',
  'pass',
  'pwd',
  'passcode',
];

/**
 * Headers that carry the browser's cookies, compared in lowercase. A cookie header's value holds `;`, `=` and quotes, and runs to the end of its field.
 */
export const COOKIE_HEADERS: readonly string[] = ['cookie', 'set-cookie'];

/**
 * Headers that carry a sign-in, compared in lowercase. Their value holds spaces and, for `Digest`, `name=value` pairs of its own, and runs to the end of its field.
 */
export const AUTHORIZATION_HEADERS: readonly string[] = ['authorization', 'proxy-authorization'];

/**
 * Authorization schemes whose next word is a credential, compared in lowercase. `Digest` is not one: its credential is a list of named pairs.
 */
export const AUTH_SCHEMES: readonly string[] = ['bearer', 'basic', 'negotiate', 'ntlm'];

/**
 * Shortest word after a scheme other than `Bearer` that is taken for a credential.
 */
export const CREDENTIAL_MIN_CHARS = 8;

/**
 * How services start their keys, compared as written: those of Stripe, Anthropic, `OpenAI`, `GitHub`, `GitLab`, npm, Slack, AWS and Google.
 */
export const KEY_PREFIXES: readonly string[] = [
  'sk_live_',
  'sk_test_',
  'sk-live-',
  'sk-test-',
  'pk_live_',
  'pk_test_',
  'rk_live_',
  'rk_test_',
  'sk-ant-',
  'sk-proj-',
  'ghp_',
  'gho_',
  'ghu_',
  'ghs_',
  'ghr_',
  'github_pat_',
  'glpat-',
  'npm_',
  'xoxa-',
  'xoxb-',
  'xoxp-',
  'xoxr-',
  'xoxs-',
  'AKIA',
  'ASIA',
  'AIza',
];

/**
 * Fewest characters after a key prefix for a word to be taken for a key, so prose that starts the same way (`AKIA` alone) is kept.
 */
export const KEY_MIN_REST = 16;

/**
 * The protocol version that added each call (D32): a client sends a call only on a connection whose version has it.
 */
export const CALL_SINCE: Readonly<Record<Call['method'], number>> = {
  add_download: 1,
  get_extension_settings: 1,
  hello: 1,
  list_queues: 1,
  pause_all: 1,
  resume_all: 1,
  show_window: 1,
  update_extension_settings: 1,
  withdraw_download: 1,
};

/**
 * The protocol version that added each event (D32): the app sends an event only on a connection whose version has it.
 */
export const EVENT_SINCE: Readonly<Record<Event['type'], number>> = {
  extension_settings_changed: 1,
  queues_changed: 1,
};

/**
 * The protocol version that added each error code (D32): the app sends a connection only codes its version has.
 */
export const ERROR_CODE_SINCE: Readonly<Record<ErrorCode, number>> = {
  addon_required: 1,
  app_not_running: 1,
  app_unavailable: 1,
  internal: 1,
  invalid_request: 1,
  invalid_url: 1,
  limit_reached: 2,
  not_found: 1,
  not_started: 1,
  unsupported_call: 2,
  unsupported_version: 1,
  update_check_failed: 2,
  wrong_state: 2,
};

/**
 * The formats a video can be saved as, the original first, in the order a menu lists
 * them (`OutputFormat::saves_video`).
 */
export const VIDEO_OUTPUT_FORMATS: readonly OutputFormat[] = ['original', 'mp4', 'mkv', 'webm'];

/**
 * The formats audio alone can be saved as, the original first, in the order a menu
 * lists them (`OutputFormat::saves_audio`).
 */
export const AUDIO_OUTPUT_FORMATS: readonly OutputFormat[] = [
  'original',
  'mp3',
  'm4a',
  'opus',
  'flac',
  'wav',
];

/**
 * The formats that keep audio without loss, so a lossy source gains nothing from
 * them (`OutputFormat::is_lossless`).
 */
export const LOSSLESS_OUTPUT_FORMATS: readonly OutputFormat[] = ['flac', 'wav'];

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
  ask_before_download: true,
  capture_enabled: true,
  capture_extensions: [],
  default_queue: DEFAULT_QUEUE_ID,
  ignored_sites: [],
  min_capture_size: 0,
  show_media_button: true,
  video_sites: [],
};
