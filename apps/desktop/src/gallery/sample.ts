// Sample values for the gallery's specimens: what a download manager would show, to judge the look by.
import type { ProgressState } from '@wirecatch/ui';

/** Stands in for the accent colour Windows would report (its default blue), so the system accent shows in the gallery. */
export const SYSTEM_ACCENT_SAMPLE = '#0078d4';

export interface SampleDownload {
  name: string;
  detail: string;
  value: number;
  state: ProgressState;
}

export const ACTIVE_DOWNLOAD: SampleDownload = {
  name: 'ubuntu-24.04.3-desktop-amd64.iso',
  detail: '2.1 GB of 5.9 GB · 18.4 MB/s · 3 min left',
  value: 0.36,
  state: 'active',
};

export const FINISHED_DOWNLOAD: SampleDownload = {
  name: 'installer.msi',
  detail: '84 MB · finished today',
  value: 1,
  state: 'done',
};

export const DOWNLOADS: readonly SampleDownload[] = [
  ACTIVE_DOWNLOAD,
  {
    name: 'conference-keynote.mp4',
    detail: '412 MB of 1.2 GB · paused',
    value: 0.34,
    state: 'paused',
  },
  {
    name: 'dataset-2026-q3.tar.zst',
    detail: 'The server stopped answering',
    value: 0.71,
    state: 'error',
  },
  FINISHED_DOWNLOAD,
  { name: 'report-final.pdf', detail: '1.4 MB · waiting in the queue', value: 0, state: 'queued' },
];

/** A part-way value for single specimens. */
export const PART_WAY = 0.62;

export const CONNECTIONS = { min: 1, max: 16, initial: 8 } as const;
export const SPEED_LIMIT = { step: 100, initial: 500 } as const;
export const BANDWIDTH_PERCENT = 60;
/** Where the moving sample bar starts. */
export const MOVING_START = 0.12;
/** A count for the solid neutral badge. */
export const SAMPLE_COUNT = 12;
/** Rows in the loading specimen. */
export const SKELETON_ROWS = [1, 2, 3] as const;
