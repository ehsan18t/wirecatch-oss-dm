// Sample data for the mock backend: believable files, queues and settings.

import type {
  AppSettings,
  Category,
  CategoryRule,
  DownloadStatus,
  Queue,
  UserError,
} from '@wirecatch/protocol';

export const HOME = 'C:\\Users\\you';
export const DOWNLOADS = `${HOME}\\Downloads`;

const GB = 1024 ** 3;
const MB = 1024 ** 2;
const KB = 1024;

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
    connections_per_download: 8,
    max_concurrent_downloads: 4,
    global_speed_limit: null,
    extension: {
      capture_enabled: true,
      ask_before_download: false,
      show_media_button: true,
      ignored_sites: ['mail.example.com'],
      capture_extensions: [],
      min_capture_size: 0,
      default_queue: 1,
    },
    theme: 'system',
    start_with_system: true,
    close_to_tray: true,
    notify_on_complete: true,
  };
}

export function defaultQueues(): Queue[] {
  return [
    {
      id: 1,
      name: 'Main',
      max_concurrent: 3,
      speed_limit: null,
      schedule: null,
      is_default: true,
      paused: false,
      running_outside_schedule: false,
    },
    {
      id: 2,
      name: 'Overnight',
      max_concurrent: 2,
      speed_limit: 4 * MB,
      schedule: { start: { hour: 1, minute: 0 }, stop: { hour: 7, minute: 0 }, days: [] },
      is_default: false,
      paused: false,
      running_outside_schedule: false,
    },
    {
      id: 3,
      name: 'Work files',
      max_concurrent: 1,
      speed_limit: 1 * MB,
      schedule: null,
      is_default: false,
      paused: false,
      running_outside_schedule: false,
    },
  ];
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

/** One hand-written sample download. Sizes are in bytes; `rate` is how fast it can go. */
export interface Seed {
  name: string;
  url: string;
  total: number | null;
  fraction: number;
  status: DownloadStatus;
  queue: number;
  rate: number;
  error?: UserError;
  /** Hours ago it was added. */
  age: number;
  connections?: number;
}

export const SEEDS: readonly Seed[] = [
  {
    name: 'ubuntu-24.04.3-desktop-amd64.iso',
    url: 'https://releases.ubuntu.com/24.04.3/ubuntu-24.04.3-desktop-amd64.iso',
    total: 6.1 * GB,
    fraction: 0.38,
    status: 'downloading',
    queue: 1,
    rate: 14 * MB,
    age: 0.4,
  },
  {
    name: 'blender-4.5.3-windows-x64.msi',
    url: 'https://download.blender.org/release/Blender4.5/blender-4.5.3-windows-x64.msi',
    total: 382 * MB,
    fraction: 0.71,
    status: 'downloading',
    queue: 1,
    rate: 6.5 * MB,
    age: 0.2,
  },
  {
    name: 'Distributed Systems, lecture 12.mp4',
    url: 'https://media.example.edu/courses/cs451/lecture-12.mp4',
    total: 1.4 * GB,
    fraction: 0.12,
    status: 'connecting',
    queue: 1,
    rate: 3.2 * MB,
    age: 0.1,
  },
  {
    name: 'live-session-recording.ts',
    url: 'https://stream.example.org/archive/session-0419.ts',
    total: null,
    fraction: 0,
    status: 'queued',
    queue: 1,
    rate: 1.1 * MB,
    age: 0.05,
  },
  {
    name: 'LibreOffice_25.8.2_Win_x86-64.msi',
    url: 'https://download.documentfoundation.org/libreoffice/stable/25.8.2/win/x86_64/LibreOffice_25.8.2_Win_x86-64.msi',
    total: 348 * MB,
    fraction: 0,
    status: 'queued',
    queue: 1,
    rate: 9 * MB,
    age: 0.05,
  },
  {
    name: 'android-studio-2026.1.1-windows.exe',
    url: 'https://redirector.gvt1.com/edgedl/android/studio/install/2026.1.1/android-studio-2026.1.1-windows.exe',
    total: 1.2 * GB,
    fraction: 0.46,
    status: 'paused',
    queue: 1,
    rate: 11 * MB,
    age: 5,
  },
  {
    name: 'Conference keynote 2026.mp4',
    url: 'https://cdn.example-conf.com/v/keynote-2026.mp4?token=e3f1',
    total: 2.3 * GB,
    fraction: 0.62,
    status: 'failed',
    queue: 1,
    rate: 8 * MB,
    age: 26,
    error: {
      message:
        'The download link expired. Get a fresh link from the page it came from; the 62% already downloaded is kept.',
      fix: 'refresh_link',
    },
  },
  {
    name: 'win11-dev-environment.zip',
    url: 'https://aka.example.com/windev_VM_hyperv.zip',
    total: 21.6 * GB,
    fraction: 0.43,
    status: 'failed',
    queue: 1,
    rate: 18 * MB,
    age: 30,
    error: {
      message: 'Drive C: is full. Free up at least 12.3 GB, then try again.',
      fix: 'free_disk_space',
    },
  },
  {
    name: 'climate-dataset-2026.csv.gz',
    url: 'https://data.example.org/climate/2026/full.csv.gz',
    total: 840 * MB,
    fraction: 0.08,
    status: 'failed',
    queue: 3,
    rate: 2 * MB,
    age: 3,
    error: {
      message:
        'Wirecatch cannot save to E:\\Backups because that drive is not connected. Choose another folder.',
      fix: 'choose_folder',
    },
  },
  {
    name: 'nightly-build-2026-10-10.tar.xz',
    url: 'https://builds.example.dev/nightly/2026-10-10.tar.xz',
    total: 512 * MB,
    fraction: 0.27,
    status: 'failed',
    queue: 1,
    rate: 5 * MB,
    age: 9,
    error: {
      message: 'The server stopped responding. Try again; the download picks up where it stopped.',
      fix: 'retry',
    },
  },
  {
    name: 'press-kit-old.zip',
    url: 'https://example.com/media/press-kit-2019.zip',
    total: null,
    fraction: 0,
    status: 'failed',
    queue: 1,
    rate: 4 * MB,
    age: 50,
    error: {
      message:
        'The file is no longer on the server (error 404). Check the page it came from for a new link.',
      fix: null,
    },
  },
  {
    name: 'old-installer-draft.exe',
    url: 'https://example.com/builds/old-installer-draft.exe',
    total: 220 * MB,
    fraction: 0,
    status: 'cancelled',
    queue: 1,
    rate: 5 * MB,
    age: 12,
  },
  {
    name: 'debian-13.1.0-amd64-DVD-1.iso',
    url: 'https://cdimage.debian.org/debian-cd/current/amd64/iso-dvd/debian-13.1.0-amd64-DVD-1.iso',
    total: 3.9 * GB,
    fraction: 0,
    status: 'queued',
    queue: 2,
    rate: 4 * MB,
    age: 2,
  },
  {
    name: 'earth-at-night-8k.mp4',
    url: 'https://images.example.gov/earth-at-night-8k.mp4',
    total: 4.4 * GB,
    fraction: 0,
    status: 'queued',
    queue: 2,
    rate: 4 * MB,
    age: 2,
  },
  {
    name: 'model-weights-shard-03-of-08.safetensors',
    url: 'https://models.example.ai/release/shard-03-of-08.safetensors',
    total: 9.8 * GB,
    fraction: 0,
    status: 'queued',
    queue: 2,
    rate: 4 * MB,
    age: 2,
  },
  {
    name: 'Q3 board pack.pdf',
    url: 'https://intranet.example.com/files/q3-board-pack.pdf',
    total: 18.4 * MB,
    fraction: 0.55,
    status: 'downloading',
    queue: 3,
    rate: 0.9 * MB,
    age: 0.3,
  },
  {
    name: 'client-assets-final.zip',
    url: 'https://share.example.com/s/client-assets-final.zip',
    total: 740 * MB,
    fraction: 0,
    status: 'queued',
    queue: 3,
    rate: 1 * MB,
    age: 0.3,
  },
  ...completedSeeds(),
];

function completedSeeds(): Seed[] {
  const done: Array<[string, string, number]> = [
    [
      'VSCodeUserSetup-x64-1.105.1.exe',
      'https://update.code.example.com/VSCodeUserSetup.exe',
      104 * MB,
    ],
    ['The Art of Unix Programming.epub', 'https://books.example.org/taoup.epub', 2.1 * MB],
    ['podcast-episode-212.mp3', 'https://feeds.example.fm/ep212.mp3', 61 * MB],
    ['podcast-episode-211.mp3', 'https://feeds.example.fm/ep211.mp3', 58 * MB],
    ['iceland-trip-photos.zip', 'https://photos.example.com/share/iceland.zip', 2.7 * GB],
    ['Invoice 1043.pdf', 'https://billing.example.com/invoice/1043.pdf', 212 * KB],
    ['Firefox Setup 143.0.exe', 'https://download.example-mozilla.org/firefox-143.exe', 78 * MB],
    ['node-v24.10.0-x64.msi', 'https://nodejs.org/dist/v24.10.0/node-v24.10.0-x64.msi', 31 * MB],
    ['wallpaper-dunes-5120x2880.jpg', 'https://images.example.com/dunes-5k.jpg', 9.6 * MB],
    ['Rust in Action, sample chapter.pdf', 'https://books.example.com/rust-sample.pdf', 4.4 * MB],
    ['field-recording-rain.flac', 'https://sounds.example.org/rain.flac', 142 * MB],
    ['7z2501-x64.exe', 'https://www.7-zip.org/a/7z2501-x64.exe', 1.6 * MB],
    ['sales-2026-09.xlsx', 'https://intranet.example.com/files/sales-2026-09.xlsx', 1.2 * MB],
    [
      'kubuntu-24.04.3-desktop-amd64.iso',
      'https://cdimage.example.com/kubuntu-24.04.3.iso',
      4.3 * GB,
    ],
    ['team-offsite-highlights.mov', 'https://share.example.com/s/offsite.mov', 880 * MB],
    ['OBS-Studio-32.0-Windows-Installer.exe', 'https://cdn.example-obs.com/OBS-32.0.exe', 152 * MB],
    ['icon-set.svg', 'https://assets.example.com/icon-set.svg', 380 * KB],
    ['fonts-collection.7z', 'https://fonts.example.com/collection.7z', 96 * MB],
  ];
  return done.map(([name, url, total], i) => ({
    name,
    url,
    total,
    fraction: 1,
    status: 'completed' as const,
    queue: i % 5 === 4 ? 3 : 1,
    rate: 0,
    age: 6 + i * 7,
  }));
}

const BULK_NAMES = [
  ['sensor-log', 'csv.gz'],
  ['episode', 'mp4'],
  ['track', 'flac'],
  ['scan', 'pdf'],
  ['backup', 'zip'],
  ['photo', 'jpg'],
  ['setup', 'exe'],
  ['chapter', 'epub'],
  ['clip', 'webm'],
  ['dataset-part', 'tar.xz'],
] as const;

/** A generated name and URL for bulk sample data, stable for a given index. */
export function bulkItem(i: number): { name: string; url: string } {
  const [stem, ext] = BULK_NAMES[i % BULK_NAMES.length] ?? ['file', 'bin'];
  const name = `${stem}-${String(i).padStart(5, '0')}.${ext}`;
  return { name, url: `https://files.example.net/bulk/${name}` };
}
