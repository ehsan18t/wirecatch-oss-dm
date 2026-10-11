// The hand-written sample downloads: believable files in every status, with every kind of fix.

import type { DownloadStatus, UserError } from '@wirecatch/protocol';
import { GB, KB, MB } from '@wirecatch/units';
import { MAIN_QUEUE_ID, OVERNIGHT_QUEUE_ID, WORK_QUEUE_ID } from './data';

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

/** Finished samples: name, link and size. */
const COMPLETED: readonly (readonly [string, string, number])[] = [
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

/** Every fifth finished sample went through the Work files queue, the rest through Main. */
const WORK_QUEUE_EVERY = 5;
/** Hours since the newest finished sample was added, and between each one and the next older. */
const COMPLETED_NEWEST_AGE_HOURS = 6;
const COMPLETED_AGE_STEP_HOURS = 7;

function completedSeeds(): Seed[] {
  return COMPLETED.map(([name, url, total], i) => ({
    name,
    url,
    total,
    fraction: 1,
    status: 'completed' as const,
    queue: i % WORK_QUEUE_EVERY === WORK_QUEUE_EVERY - 1 ? WORK_QUEUE_ID : MAIN_QUEUE_ID,
    rate: 0,
    age: COMPLETED_NEWEST_AGE_HOURS + i * COMPLETED_AGE_STEP_HOURS,
  }));
}

export const SEEDS: readonly Seed[] = [
  {
    name: 'ubuntu-24.04.3-desktop-amd64.iso',
    url: 'https://releases.ubuntu.com/24.04.3/ubuntu-24.04.3-desktop-amd64.iso',
    total: 6.1 * GB,
    fraction: 0.38,
    status: 'downloading',
    queue: MAIN_QUEUE_ID,
    rate: 14 * MB,
    age: 0.4,
  },
  {
    name: 'blender-4.5.3-windows-x64.msi',
    url: 'https://download.blender.org/release/Blender4.5/blender-4.5.3-windows-x64.msi',
    total: 382 * MB,
    fraction: 0.71,
    status: 'downloading',
    queue: MAIN_QUEUE_ID,
    rate: 6.5 * MB,
    age: 0.2,
  },
  {
    name: 'Distributed Systems, lecture 12.mp4',
    url: 'https://media.example.edu/courses/cs451/lecture-12.mp4',
    total: 1.4 * GB,
    fraction: 0.12,
    status: 'connecting',
    queue: MAIN_QUEUE_ID,
    rate: 3.2 * MB,
    age: 0.1,
  },
  {
    name: 'live-session-recording.ts',
    url: 'https://stream.example.org/archive/session-0419.ts',
    total: null,
    fraction: 0,
    status: 'queued',
    queue: MAIN_QUEUE_ID,
    rate: 1.1 * MB,
    age: 0.05,
  },
  {
    name: 'LibreOffice_25.8.2_Win_x86-64.msi',
    url: 'https://download.documentfoundation.org/libreoffice/stable/25.8.2/win/x86_64/LibreOffice_25.8.2_Win_x86-64.msi',
    total: 348 * MB,
    fraction: 0,
    status: 'queued',
    queue: MAIN_QUEUE_ID,
    rate: 9 * MB,
    age: 0.05,
  },
  {
    name: 'android-studio-2026.1.1-windows.exe',
    url: 'https://redirector.gvt1.com/edgedl/android/studio/install/2026.1.1/android-studio-2026.1.1-windows.exe',
    total: 1.2 * GB,
    fraction: 0.46,
    status: 'paused',
    queue: MAIN_QUEUE_ID,
    rate: 11 * MB,
    age: 5,
  },
  {
    name: 'Conference keynote 2026.mp4',
    url: 'https://cdn.example-conf.com/v/keynote-2026.mp4?token=e3f1',
    total: 2.3 * GB,
    fraction: 0.62,
    status: 'failed',
    queue: MAIN_QUEUE_ID,
    rate: 8 * MB,
    age: 26,
    error: {
      message:
        'The download link expired. Get a fresh link from the page it came from; the 62% already downloaded is kept.',
      fix: 'refresh_link',
    },
  },
  {
    name: 'Northern lights over Tromso, 4K time-lapse.mp4',
    url: 'https://video.example.com/watch/aurora-tromso',
    total: 1.8 * GB,
    fraction: 1,
    status: 'failed',
    queue: MAIN_QUEUE_ID,
    rate: 9 * MB,
    age: 28,
    error: {
      message:
        'Wirecatch downloaded the video, but could not save it as MP4 (Conversion failed). The download is kept: Try again only converts it again, and Keep the original saves it as it was downloaded (WEBM).',
      fix: 'keep_original',
    },
  },
  {
    name: 'win11-dev-environment.zip',
    url: 'https://aka.example.com/windev_VM_hyperv.zip',
    total: 21.6 * GB,
    fraction: 0.43,
    status: 'failed',
    queue: MAIN_QUEUE_ID,
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
    queue: WORK_QUEUE_ID,
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
    queue: MAIN_QUEUE_ID,
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
    queue: MAIN_QUEUE_ID,
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
    queue: MAIN_QUEUE_ID,
    rate: 5 * MB,
    age: 12,
  },
  {
    name: 'debian-13.1.0-amd64-DVD-1.iso',
    url: 'https://cdimage.debian.org/debian-cd/current/amd64/iso-dvd/debian-13.1.0-amd64-DVD-1.iso',
    total: 3.9 * GB,
    fraction: 0,
    status: 'queued',
    queue: OVERNIGHT_QUEUE_ID,
    rate: 4 * MB,
    age: 2,
  },
  {
    name: 'earth-at-night-8k.mp4',
    url: 'https://images.example.gov/earth-at-night-8k.mp4',
    total: 4.4 * GB,
    fraction: 0,
    status: 'queued',
    queue: OVERNIGHT_QUEUE_ID,
    rate: 4 * MB,
    age: 2,
  },
  {
    name: 'model-weights-shard-03-of-08.safetensors',
    url: 'https://models.example.ai/release/shard-03-of-08.safetensors',
    total: 9.8 * GB,
    fraction: 0,
    status: 'queued',
    queue: OVERNIGHT_QUEUE_ID,
    rate: 4 * MB,
    age: 2,
  },
  {
    name: 'Q3 board pack.pdf',
    url: 'https://intranet.example.com/files/q3-board-pack.pdf',
    total: 18.4 * MB,
    fraction: 0.55,
    status: 'downloading',
    queue: WORK_QUEUE_ID,
    rate: 0.9 * MB,
    age: 0.3,
  },
  {
    name: 'client-assets-final.zip',
    url: 'https://share.example.com/s/client-assets-final.zip',
    total: 740 * MB,
    fraction: 0,
    status: 'queued',
    queue: WORK_QUEUE_ID,
    rate: 1 * MB,
    age: 0.3,
  },
  ...completedSeeds(),
];

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

/** Digits in a generated name's number, so the names sort in order (`episode-00042.mp4`). */
const BULK_NUMBER_DIGITS = 5;

/** A generated name and URL for bulk sample data, stable for a given index. */
export function bulkItem(i: number): { name: string; url: string } {
  const [stem, ext] = BULK_NAMES[i % BULK_NAMES.length] ?? ['file', 'bin'];
  const name = `${stem}-${String(i).padStart(BULK_NUMBER_DIGITS, '0')}.${ext}`;
  return { name, url: `https://files.example.net/bulk/${name}` };
}
