// Sample notices for the notices preview, in the shapes the app sends (`alerts.rs` writes the real ones).
import { noticeId, type BrowserNotice } from '@wirecatch/protocol';

const SAMPLES = ['started', 'finished', 'failed', 'several'] as const;
export type SampleNotices = (typeof SAMPLES)[number];

export function isSampleNotices(value: unknown): value is SampleNotices {
  return SAMPLES.some((sample) => sample === value);
}

let lastId = 0;
/** A notice id no other sample notice has. */
const nextId = () => noticeId(++lastId);

const STARTED: BrowserNotice = {
  id: nextId(),
  kind: 'started',
  title: 'Download added',
  body: 'Conference keynote 2026, reading the video',
  buttons: [],
  stays_ms: null,
};

const FINISHED: BrowserNotice = {
  id: nextId(),
  kind: 'finished',
  title: 'Download finished',
  body: 'ubuntu-26.04-desktop-amd64.iso',
  buttons: ['Show in folder'],
  stays_ms: null,
};

const FAILED: BrowserNotice = {
  id: nextId(),
  kind: 'failed',
  title: 'Download failed',
  body: 'report-final.pdf: The server refused the download.',
  buttons: ['Try again'],
  stays_ms: null,
};

/** The notices each sample shows; they stay, so the preview can be looked at. */
export function sampleNotices(which: SampleNotices): BrowserNotice[] {
  switch (which) {
    case 'started':
      return [STARTED];
    case 'finished':
      return [FINISHED];
    case 'failed':
      return [FAILED];
    case 'several':
      return [STARTED, FINISHED, FAILED];
  }
}
