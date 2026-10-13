import { downloadId, queueId, type DownloadSummary } from '@wirecatch/protocol';
import { describe, expect, it } from 'vitest';
import {
  earlierNote,
  FIX_LABEL,
  extractUrls,
  fileNameFromUrl,
  folderOf,
  formatDays,
  formatEta,
  formatPercent,
  formatSchedule,
  formatSize,
  linkForDisplay,
  parseTime,
  percentOf,
  processingFraction,
  progressbarValue,
  wireFraction,
  roundDownTo,
  shownWait,
  scheduleAllows,
  statusDetail,
  statusShort,
} from './format';

describe('time', () => {
  it('formats time left compactly', () => {
    expect(formatEta(null)).toBe('');
    expect(formatEta(0.2)).toBe('1 s');
    expect(formatEta(45)).toBe('45 s');
    expect(formatEta(180)).toBe('3 min');
    expect(formatEta(3600)).toBe('1 h');
    expect(formatEta(4800)).toBe('1 h 20 min');
    expect(formatEta(3 * 86400)).toBe('3 days');
  });
});

describe('progress', () => {
  it('never claims 100% before the end', () => {
    expect(formatPercent(999, 1000)).toBe('99%');
    expect(formatPercent(1000, 1000)).toBe('100%');
    expect(formatPercent(5, null)).toBe('');
    expect(percentOf(0.426)).toBe(42);
    expect(percentOf(1)).toBe(99);
  });

  it('gives a progress bar its value in percent, or none while unknown', () => {
    expect(progressbarValue(0.5)).toEqual({
      'aria-valuemin': 0,
      'aria-valuemax': 100,
      'aria-valuenow': 50,
    });
    expect(progressbarValue(1)['aria-valuenow']).toBe(99);
    expect(progressbarValue(1, true)['aria-valuenow']).toBe(100);
    expect(progressbarValue(null)['aria-valuenow']).toBeUndefined();
  });

  it('fills a download wire one way everywhere, moving while how far is not known', () => {
    const at = (status: DownloadSummary['status'], done: number, total: number | null) => ({
      status,
      done,
      total,
      processing: null,
    });
    expect(wireFraction(at('downloading', 25, 100))).toBe(0.25);
    expect(wireFraction(at('downloading', 25, null))).toBeNull();
    expect(wireFraction(at('connecting', 0, null))).toBeNull();
    expect(wireFraction(at('paused', 25, null))).toBe(0);
    expect(wireFraction(at('completed', 0, null))).toBe(1);
    expect(wireFraction(at('processing', 100, 100))).toBeNull();
    expect(
      wireFraction({
        ...at('processing', 100, 100),
        processing: { step: 'joining', per_mille: 500 },
      }),
    ).toBe(0.5);
  });

  it('rounds down to a step', () => {
    expect(roundDownTo(68, 25)).toBe(50);
    expect(roundDownTo(100, 25)).toBe(100);
    expect(roundDownTo(24.9, 25)).toBe(0);
  });

  it('describes size in plain words', () => {
    expect(formatSize(0, null, 'queued')).toBe('Size not known yet');
    expect(formatSize(1024, 4096, 'downloading')).toBe('1.00 KB of 4.00 KB');
    expect(formatSize(4096, 4096, 'completed')).toBe('4.00 KB');
  });
});

describe('schedules', () => {
  const night = { start: { hour: 1, minute: 0 }, stop: { hour: 7, minute: 0 }, days: [] };
  const lateNight = { start: { hour: 22, minute: 0 }, stop: { hour: 6, minute: 0 }, days: [] };

  it('reads like a sentence', () => {
    expect(formatSchedule(null)).toBe('Runs any time');
    expect(formatSchedule(night)).toBe('01:00 to 07:00');
    expect(formatSchedule({ ...night, days: ['sat', 'sun'] })).toBe('01:00 to 07:00, weekends');
    expect(formatSchedule({ start: { hour: 9, minute: 30 }, stop: null, days: [] })).toBe(
      'From 09:30 until empty',
    );
    expect(formatDays(['mon', 'tue', 'wed', 'thu', 'fri'])).toBe('Weekdays');
    expect(formatDays(['fri', 'mon'])).toBe('Mon, Fri');
  });

  it('knows when a queue may run, including windows over midnight', () => {
    expect(scheduleAllows(night, new Date(2026, 9, 10, 3, 0))).toBe(true);
    expect(scheduleAllows(night, new Date(2026, 9, 10, 12, 0))).toBe(false);
    expect(scheduleAllows(lateNight, new Date(2026, 9, 10, 23, 0))).toBe(true);
    expect(scheduleAllows(lateNight, new Date(2026, 9, 10, 5, 0))).toBe(true);
    expect(scheduleAllows(lateNight, new Date(2026, 9, 10, 12, 0))).toBe(false);
    // 2026-10-10 is a Saturday.
    expect(scheduleAllows({ ...night, days: ['mon'] }, new Date(2026, 9, 10, 3, 0))).toBe(false);
  });

  it('parses times strictly', () => {
    expect(parseTime('07:05')).toEqual({ hour: 7, minute: 5 });
    expect(parseTime('24:00')).toBeNull();
    expect(parseTime('soon')).toBeNull();
  });
});

describe('links', () => {
  it('pulls every link out of pasted text', () => {
    const text =
      'Get it here: https://example.com/a.zip, and (https://example.com/b.iso).\nftp://x.org/c';
    expect(extractUrls(text)).toEqual([
      'https://example.com/a.zip',
      'https://example.com/b.iso',
      'ftp://x.org/c',
    ]);
    expect(extractUrls('no links here')).toEqual([]);
    expect(extractUrls('https://a.com/x https://a.com/x')).toHaveLength(1);
  });

  it('never shows or copies a password in a link', () => {
    expect(linkForDisplay('https://ada:s3cret@example.com/a.zip')).toBe(
      'https://ada:***@example.com/a.zip',
    );
    expect(linkForDisplay('ftp://ada:p@ss@files.example.com:21/x?y=a@b')).toBe(
      'ftp://ada:***@files.example.com:21/x?y=a@b',
    );
    expect(linkForDisplay('https://token@example.com/a')).toBe('https://***@example.com/a');
    expect(linkForDisplay('https://example.com/a@b?c=d@e')).toBe('https://example.com/a@b?c=d@e');
    expect(linkForDisplay('not a link')).toBe('not a link');
  });

  it('guesses file names and folders', () => {
    expect(fileNameFromUrl('https://example.com/dir/My%20File.pdf?x=1')).toBe('My File.pdf');
    expect(fileNameFromUrl('https://example.com/')).toBe('example.com');
    expect(folderOf('C:\\Users\\you\\Downloads\\a.zip')).toBe('C:\\Users\\you\\Downloads');
    expect(folderOf('/home/you/a.zip')).toBe('/home/you');
  });
});

describe('a download already in the list (D23)', () => {
  const now = new Date(2026, 9, 10, 20, 0);
  const row = (change: Partial<DownloadSummary>): DownloadSummary => ({
    id: downloadId(1),
    url: 'https://example.com/a.zip',
    file_name: 'a.zip',
    path: 'C:/Downloads/a.zip',
    queue: queueId(1),
    category: 'archive',
    status: 'completed',
    total: 10,
    done: 10,
    speed: 0,
    connections: 0,
    eta_secs: null,
    error: null,
    waiting: null,
    media: null,
    processing: null,
    added_at: new Date(2026, 9, 10, 18, 30).getTime(),
    completed_at: new Date(2026, 9, 10, 18, 38).getTime(),
    ...change,
  });

  it('says when it was downloaded, or what it is doing, in one line', () => {
    expect(earlierNote(row({}), now)).toBe('You downloaded this today at 18:38.');
    expect(earlierNote(row({ completed_at: new Date(2026, 9, 9, 7, 5).getTime() }), now)).toBe(
      'You downloaded this yesterday at 07:05.',
    );
    expect(earlierNote(row({ completed_at: new Date(2026, 8, 3, 9, 0).getTime() }), now)).toBe(
      'You downloaded this on 3 Sept 2026.',
    );
    expect(earlierNote(row({ status: 'downloading' }), now)).toBe('This is already downloading.');
    expect(earlierNote(row({ status: 'processing' }), now)).toBe('This is already downloading.');
    expect(earlierNote(row({ status: 'paused' }), now)).toBe(
      'This is already in your list, paused.',
    );
    expect(earlierNote(row({ status: 'cancelled' }), now)).toBe(
      'You started this today at 18:30 and cancelled it.',
    );
    for (const status of ['queued', 'failed', 'connecting'] as const) {
      expect(earlierNote(row({ status }), now)).not.toContain(String.fromCharCode(0x2014));
    }
  });

  it('describes the status as the list does', () => {
    const waiting = row({
      status: 'queued',
      waiting: { type: 'queue_full', data: { running: 2, limit: 2 } },
    });
    expect(statusShort(waiting)).toBe('Queue full');
    expect(statusDetail(waiting, 'Main')).toBe(
      'Main is running 2 of 2. This starts when one of them finishes.',
    );
    expect(statusShort(row({ status: 'processing' }))).toBe('Joining video and audio');
    expect(statusDetail(row({ status: 'processing' }), 'Main')).toMatch(/joining them/);
    expect(statusDetail(row({}), 'Main')).toBeNull();
  });

  it('names what ffmpeg does to a video and how far it is (D24)', () => {
    const converting = row({
      status: 'processing',
      processing: { step: 'converting', per_mille: 456 },
    });
    expect(statusShort(converting)).toBe('Converting, 45%');
    expect(processingFraction(converting)).toBeCloseTo(0.456);
    expect(statusDetail(converting, 'Main')).toMatch(/converting it to the format you chose/);
    const finishing = row({
      status: 'processing',
      processing: { step: 'finishing', per_mille: null },
    });
    expect(statusShort(finishing)).toBe('Finishing the file');
    expect(processingFraction(finishing)).toBeNull();
    // Only while processing.
    expect(processingFraction(row({ status: 'paused', processing: null }))).toBeNull();
    expect(FIX_LABEL.keep_original).toBe('Keep the original');
  });
});

describe('a video whose server refused it for now', () => {
  const at = new Date(2026, 9, 11, 20, 51).getTime();
  const waiting = { type: 'server_refused', data: { retry_at: at } } as const;

  it('says when the next try is while connecting, not that it failed', () => {
    const row = { status: 'connecting', waiting } as const;
    expect(shownWait(row)).toEqual(waiting);
    expect(statusShort(row)).toBe('Retrying at 20:51');
    const full: DownloadSummary = {
      id: downloadId(1),
      url: 'https://www.youtube.com/watch?v=x',
      file_name: 'Talk.mp4',
      path: 'C:/Downloads/Video/Talk.mp4',
      queue: queueId(1),
      category: 'video',
      status: 'connecting',
      total: null,
      done: 0,
      speed: 0,
      connections: 0,
      eta_secs: null,
      error: null,
      waiting,
      media: null,
      processing: null,
      added_at: at,
      completed_at: null,
    };
    const detail = statusDetail(full, 'Main');
    expect(detail).toContain('tries again by itself at 20:51');
    expect(detail).not.toContain('\u2014');
  });

  it('shows no stale refusal once the download moved on', () => {
    expect(shownWait({ status: 'downloading', waiting })).toBeNull();
    expect(statusShort({ status: 'downloading', waiting })).toBe('Downloading');
    expect(shownWait({ status: 'connecting', waiting: null })).toBeNull();
  });
});
