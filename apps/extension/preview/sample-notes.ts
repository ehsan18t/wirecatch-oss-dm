// Sample notes for the notes preview, in the shapes the app sends (`notices.rs` writes the real ones).
import { noteId, type BrowserNote } from '@wirecatch/protocol';

const SAMPLES = ['started', 'finished', 'failed', 'several'] as const;
export type SampleNotes = (typeof SAMPLES)[number];

export function isSampleNotes(value: unknown): value is SampleNotes {
  return SAMPLES.some((sample) => sample === value);
}

let lastId = 0;
/** A note id no other sample note has. */
const nextId = () => noteId(++lastId);

const STARTED: BrowserNote = {
  id: nextId(),
  kind: 'started',
  title: 'Download added',
  body: 'Conference keynote 2026, reading the video',
  buttons: [],
  stays_ms: null,
};

const FINISHED: BrowserNote = {
  id: nextId(),
  kind: 'finished',
  title: 'Download finished',
  body: 'ubuntu-26.04-desktop-amd64.iso',
  buttons: ['Show in folder'],
  stays_ms: null,
};

const FAILED: BrowserNote = {
  id: nextId(),
  kind: 'failed',
  title: 'Download failed',
  body: 'report-final.pdf: The server refused the download.',
  buttons: ['Try again'],
  stays_ms: null,
};

/** The notes each sample shows; they stay, so the preview can be looked at. */
export function sampleNotes(which: SampleNotes): BrowserNote[] {
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
