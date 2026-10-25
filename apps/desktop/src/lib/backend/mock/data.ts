// Sample settings, queues and folders for the mock backend, set up as on a believable Windows machine.

import {
  DEFAULT_CONNECTIONS_PER_DOWNLOAD,
  DEFAULT_MAX_CONCURRENT_DOWNLOADS,
  DEFAULT_QUEUE_ID,
  FOLLOW_APP,
  MANUAL_QUEUE,
  ownSetting,
  queueId,
  type AddonInfo,
  type AppSettings,
  type Category,
  type CategoryRule,
  type Queue,
  type QueueStatus,
  type StartupNotice,
  type SuffixListInfo,
} from '@wirecatch/protocol';
import { PRODUCT_NAME, PRODUCT_SLUG } from '@wirecatch/product';
import { MB } from '@wirecatch/units';
import { defaultProxySettings } from '../../proxy';

export const HOME = String.raw`C:\Users\you`;
export const DOWNLOADS = String.raw`${HOME}\Downloads`;

/** What the folder picker offers, one after another. The last is long, for the layout checks; later picks reach it. */
export const PICKED_FOLDERS: readonly string[] = [
  String.raw`${HOME}\Videos`,
  String.raw`${DOWNLOADS}\Later`,
  String.raw`D:\Media`,
  String.raw`${HOME}\Desktop`,
  String.raw`D:\Projects\Customer deliveries\2026\October\Raw footage from the second shoot, unedited and not yet sorted`,
];

const MEDIA_ADDON: AddonInfo = {
  id: 'media',
  // As the core describes it.
  name: 'Media add-on',
  description:
    'Downloads video and audio from sites such as YouTube (yt-dlp), and joins separate video and audio into one file (ffmpeg).',
  state: { type: 'not_installed' },
  installed_version: null,
  latest_version: 'yt-dlp 2026.08.19, ffmpeg 9.0.2, Deno 2.9.7',
  size: 131 * MB,
  note: null,
};

/** The Public Suffix List built into the sample app, published on 15 September 2026 (D44). */
export function builtInSuffixList(): SuffixListInfo {
  return { published: Date.parse('2026-09-15T10:18:26Z'), checked: null, check_failed: false };
}

/** The one optional add-on (D20): yt-dlp, ffmpeg and Deno, installed together. */
export function mediaAddon(): AddonInfo {
  return structuredClone(MEDIA_ADDON);
}

/** The sample queues' ids. */
export const MAIN_QUEUE_ID = DEFAULT_QUEUE_ID;
export const OVERNIGHT_QUEUE_ID = queueId(2);
export const WORK_QUEUE_ID = queueId(OVERNIGHT_QUEUE_ID + 1);

export function defaultCategoryRules(): CategoryRule[] {
  return [
    {
      category: 'video',
      extensions: ['mp4', 'mkv', 'webm', 'mov', 'avi', 'ts'],
      folder: '{Downloads}/Video',
    },
    {
      category: 'audio',
      extensions: ['mp3', 'flac', 'm4a', 'wav', 'ogg', 'opus'],
      folder: '{Downloads}/Music',
    },
    {
      category: 'archive',
      extensions: ['zip', 'rar', '7z', 'tar', 'gz', 'xz', 'iso'],
      folder: '{Downloads}/Archives',
    },
    {
      category: 'document',
      extensions: ['pdf', 'docx', 'xlsx', 'pptx', 'txt', 'epub', 'csv'],
      folder: '{Downloads}/Documents',
    },
    {
      category: 'program',
      extensions: ['exe', 'msi', 'deb', 'rpm', 'appimage', 'dmg'],
      folder: '{Downloads}/Programs',
    },
    {
      category: 'image',
      extensions: ['jpg', 'jpeg', 'png', 'webp', 'gif', 'svg'],
      folder: '{Downloads}/Pictures',
    },
  ];
}

export function defaultSettings(): AppSettings {
  return {
    default_folder: '{Downloads}',
    categories: defaultCategoryRules(),
    connections_per_download: DEFAULT_CONNECTIONS_PER_DOWNLOAD,
    max_concurrent_downloads: DEFAULT_MAX_CONCURRENT_DOWNLOADS,
    global_speed_limit: null,
    extension: {
      capture_enabled: true,
      ask_before_download: true,
      show_media_button: true,
      private_windows: true,
      ignored_sites: ['mail.example.com'],
      capture_extensions: [],
      min_capture_size: 0,
      default_queue: MAIN_QUEUE_ID,
    },
    theme: 'system',
    accent: 'cyan',
    start_with_system: true,
    close_to_tray: true,
    finish_notice: 'browser',
    finish_note_time: 'eight_seconds',
    start_notice: 'browser',
    start_note_time: 'three_seconds',
    notify_during_do_not_disturb: true,
    auto_update_addons: true,
    show_progress_windows: true,
    media: {
      video: defaultOutput(),
      audio: defaultOutput(),
      ask_each_time: true,
      quality: 'best',
      fallback: 'lower',
    },
    local_sites: [],
    update_suffix_list: true,
    proxy: defaultProxySettings(),
  };
}

/** Where a quality window starts before anything was chosen (D24): the original format, and a language's auto-generated subtitles when it has no others (D50). */
export function defaultOutput(): AppSettings['media']['video'] {
  return {
    format: 'original',
    audio_kbps: null,
    subtitle_languages: [],
    auto_subtitles: true,
    subtitle_files: false,
    chapters: false,
  };
}

export const QUEUE_RUNNING: QueueStatus = { paused: false, running_outside_schedule: false };

// Unlike a fresh install, the sample's Main has its own limit: the demo needs it full while the other queues run.
const MAIN_AT_ONCE = 3;
const OVERNIGHT_SPEED_LIMIT = 4 * MB;
const WORK_CONNECTIONS = 4;

const DEFAULT_QUEUES: readonly Queue[] = [
  {
    id: MAIN_QUEUE_ID,
    is_default: true,
    settings: {
      name: 'Main',
      description: '',
      max_concurrent: ownSetting(MAIN_AT_ONCE),
      speed_limit: FOLLOW_APP,
      connections: FOLLOW_APP,
      takes: MANUAL_QUEUE,
      schedule: null,
      notify: 'each_download',
    },
    status: { ...QUEUE_RUNNING },
  },
  {
    id: OVERNIGHT_QUEUE_ID,
    is_default: false,
    settings: {
      name: 'Overnight',
      description: 'Large files, while nobody is using the connection',
      max_concurrent: ownSetting(2),
      speed_limit: ownSetting(OVERNIGHT_SPEED_LIMIT),
      connections: FOLLOW_APP,
      takes: MANUAL_QUEUE,
      schedule: { start: { hour: 1, minute: 0 }, stop: { hour: 7, minute: 0 }, days: [] },
      notify: 'when_done',
    },
    status: { ...QUEUE_RUNNING },
  },
  {
    id: WORK_QUEUE_ID,
    is_default: false,
    settings: {
      name: 'Work files',
      description: '',
      max_concurrent: ownSetting(1),
      speed_limit: ownSetting(1 * MB),
      connections: ownSetting(WORK_CONNECTIONS),
      takes: MANUAL_QUEUE,
      schedule: null,
      notify: 'each_download',
    },
    status: { ...QUEUE_RUNNING },
  },
];

/** The sample queues, as a fresh copy the mock may change. */
export function defaultQueues(): Queue[] {
  return DEFAULT_QUEUES.map((q) => structuredClone(q));
}

/** Turns `{Downloads}/Video` into a real Windows-style folder. */
export function resolvePortable(path: string): string {
  return path
    .replace('{Downloads}', () => DOWNLOADS)
    .replace('{Home}', () => HOME)
    .replace('{Documents}', () => String.raw`${HOME}\Documents`)
    .replace('{Videos}', () => String.raw`${HOME}\Videos`)
    .replace('{Music}', () => String.raw`${HOME}\Music`)
    .replace('{Pictures}', () => String.raw`${HOME}\Pictures`)
    .replaceAll('/', '\\');
}

export function categoryFor(fileName: string, rules: readonly CategoryRule[]): Category {
  const ext = fileName.includes('.') ? (fileName.split('.').pop() ?? '').toLowerCase() : '';
  for (const rule of rules) if (rule.extensions.includes(ext)) return rule.category;
  return 'other';
}

export function folderFor(category: Category, settings: AppSettings): string {
  const rule = settings.categories.find((r) => r.category === category);
  return resolvePortable(rule ? rule.folder : settings.default_folder);
}

/** The notice the core gives after setting a damaged download list aside, worded as the core words it. */
const DAMAGED_LIST_NOTICE: StartupNotice = {
  title: `${PRODUCT_NAME} started a new download list`,
  message: `The download list was damaged, so ${PRODUCT_NAME} set it aside and started an empty one. Files you already downloaded are still in their folders. The damaged list is kept at ${HOME}\\AppData\\Roaming\\${PRODUCT_SLUG}\\downloads.db.damaged-1791000000000.`,
  set_aside: true,
};

/** The notice the core gives when it could not make its data folder private, worded as the core words it. */
const NOT_PRIVATE_NOTICE: StartupNotice = {
  title: 'Other accounts may be able to read your download list',
  message: `${PRODUCT_NAME} could not make its folder ${HOME}\\AppData\\Roaming\\${PRODUCT_SLUG} private to your account (access is denied). The download list in it keeps the sign-ins websites gave for your downloads, so other accounts on this computer may be able to read them. Make sure your account owns the folder and that it is on a drive that keeps permissions, then open ${PRODUCT_NAME} again.`,
  set_aside: false,
};

/** The notices the core gives at start, in its order, for a start as `options` describe it. */
export function startupNoticesFor(options: {
  damagedList?: boolean;
  dataFolderNotPrivate?: boolean;
}): StartupNotice[] {
  return [
    ...(options.damagedList ? [DAMAGED_LIST_NOTICE] : []),
    ...(options.dataFolderNotPrivate ? [NOT_PRIVATE_NOTICE] : []),
  ];
}
