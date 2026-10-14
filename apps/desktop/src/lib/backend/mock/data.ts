// Sample settings, queues and folders for the mock backend, set up as on a believable Windows machine.

import {
  DEFAULT_CONNECTIONS_PER_DOWNLOAD,
  DEFAULT_MAX_CONCURRENT_DOWNLOADS,
  DEFAULT_QUEUE_ID,
  DEFAULT_QUEUE_MAX_CONCURRENT,
  queueId,
  type AddonInfo,
  type AppSettings,
  type Category,
  type CategoryRule,
  type Queue,
  type SuffixListInfo,
} from '@wirecatch/protocol';
import { MB } from '@wirecatch/units';

export const HOME = 'C:\\Users\\you';
export const DOWNLOADS = `${HOME}\\Downloads`;

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
      ignored_sites: ['mail.example.com'],
      capture_extensions: [],
      min_capture_size: 0,
      default_queue: MAIN_QUEUE_ID,
    },
    theme: 'system',
    accent: 'cyan',
    start_with_system: true,
    close_to_tray: true,
    notify_on_complete: true,
    auto_update_addons: true,
    show_progress_windows: true,
    media: { video: defaultOutput(), audio: defaultOutput() },
    local_sites: [],
    update_suffix_list: true,
  };
}

/** Where a quality window starts before anything was chosen (D24): the original format. */
export function defaultOutput(): AppSettings['media']['video'] {
  return {
    format: 'original',
    audio_kbps: null,
    subtitle_languages: [],
    auto_subtitles: false,
    subtitle_files: false,
    chapters: false,
  };
}

const DEFAULT_QUEUES: readonly Queue[] = [
  {
    id: MAIN_QUEUE_ID,
    name: 'Main',
    max_concurrent: DEFAULT_QUEUE_MAX_CONCURRENT,
    speed_limit: null,
    schedule: null,
    is_default: true,
    paused: false,
    running_outside_schedule: false,
  },
  {
    id: OVERNIGHT_QUEUE_ID,
    name: 'Overnight',
    max_concurrent: 2,
    speed_limit: 4 * MB,
    schedule: { start: { hour: 1, minute: 0 }, stop: { hour: 7, minute: 0 }, days: [] },
    is_default: false,
    paused: false,
    running_outside_schedule: false,
  },
  {
    id: WORK_QUEUE_ID,
    name: 'Work files',
    max_concurrent: 1,
    speed_limit: 1 * MB,
    schedule: null,
    is_default: false,
    paused: false,
    running_outside_schedule: false,
  },
];

/** The sample queues, as a fresh copy the mock may change. */
export function defaultQueues(): Queue[] {
  return DEFAULT_QUEUES.map((q) => structuredClone(q));
}

/** Turns `{Downloads}/Video` into a real Windows-style folder. */
export function resolvePortable(path: string): string {
  return path
    .replace('{Downloads}', DOWNLOADS)
    .replace('{Home}', HOME)
    .replace('{Documents}', `${HOME}\\Documents`)
    .replace('{Videos}', `${HOME}\\Videos`)
    .replace('{Music}', `${HOME}\\Music`)
    .replace('{Pictures}', `${HOME}\\Pictures`)
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
