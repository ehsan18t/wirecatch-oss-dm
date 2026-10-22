// The sample app's download windows follow the desktop's rules (D23, `download_windows.rs`).

import { describe, expect, it } from 'vitest';
import { downloadId, MAX_QUESTION_WINDOWS, questionId } from '@wirecatch/protocol';
import { WindowRegistry } from './sample-windows';

describe('WindowRegistry', () => {
  it('opens at most five question windows; the rest wait their turn', () => {
    const r = new WindowRegistry();
    for (let id = 1; id <= 7; id++) r.ask(questionId(id));
    expect(r.windows.map((w) => w.slot)).toEqual([0, 1, 2, 3, 4]);
    expect(r.windows).toHaveLength(MAX_QUESTION_WINDOWS);
    expect(r.waiting).toEqual([6, 7]);
    // Asked again (the video was read): nothing opens twice.
    r.ask(questionId(3));
    r.ask(questionId(6));
    expect(r.windows).toHaveLength(5);

    // A window that shows its download makes room and keeps its place.
    r.becameDownload('download-q2', downloadId(40));
    expect(r.windows.find((w) => w.label === 'download-q2')?.shows).toEqual({
      kind: 'download',
      id: 40,
    });
    expect(r.windows.at(-1)).toMatchObject({ shows: { kind: 'question', id: 6 }, slot: 5 });
    // A closed window makes room too, and its place is used again.
    r.gone('download-q1');
    expect(r.windows.at(-1)).toMatchObject({ shows: { kind: 'question', id: 7 }, slot: 0 });
  });

  it('never reopens a question the person put aside, until asked to', () => {
    const r = new WindowRegistry();
    r.ask(questionId(1));
    r.gone('download-q1');
    expect(r.windows).toHaveLength(0);
    r.showAll([questionId(1)]);
    expect(r.windows.map((w) => w.label)).toEqual(['download-q1']);
    r.showAll([questionId(1)]);
    expect(r.windows).toHaveLength(1);
    expect(r.front).toBe('download-q1');
  });

  it('drops a question closed while it waited', () => {
    const r = new WindowRegistry();
    for (let id = 1; id <= 6; id++) r.ask(questionId(id));
    r.closed(questionId(6));
    r.gone('download-q1');
    expect(r.windows.some((w) => w.shows.kind === 'question' && w.shows.id === 6)).toBe(false);
  });

  it('opens one progress window per download, beyond the limit', () => {
    const r = new WindowRegistry();
    for (let id = 1; id <= 5; id++) r.ask(questionId(id));
    r.showDownload(downloadId(9));
    r.showDownload(downloadId(9));
    expect(r.windows.filter((w) => w.shows.kind === 'download')).toHaveLength(1);
    expect(r.windows).toHaveLength(6);
  });

  it('tells its listeners about every change', () => {
    const r = new WindowRegistry();
    const seen: number[] = [];
    const stop = r.subscribe((windows) => seen.push(windows.length));
    r.ask(questionId(1));
    r.gone('download-q1');
    stop();
    r.ask(questionId(2));
    expect(seen).toEqual([0, 1, 0]);
  });
});
