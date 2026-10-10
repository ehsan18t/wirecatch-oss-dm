import { describe, expect, it } from 'vitest';
import {
  extractUrls,
  fileNameFromUrl,
  folderOf,
  formatBytes,
  formatDays,
  formatEta,
  formatPercent,
  formatRate,
  formatSchedule,
  formatSize,
  formatSpeed,
  parseTime,
  scheduleAllows,
} from './format';

describe('formatBytes', () => {
  it('uses short units with sensible precision', () => {
    expect(formatBytes(0)).toBe('0 B');
    expect(formatBytes(512)).toBe('512 B');
    expect(formatBytes(1536)).toBe('1.50 KB');
    expect(formatBytes(15 * 1024 * 1024)).toBe('15.0 MB');
    expect(formatBytes(6.1 * 1024 ** 3)).toBe('6.10 GB');
    expect(formatBytes(-1)).toBe('0 B');
  });
});

describe('speed, rate and time', () => {
  it('hides a zero speed', () => {
    expect(formatSpeed(0)).toBe('');
    expect(formatSpeed(2 * 1024 * 1024)).toBe('2.00 MB/s');
  });

  it('writes limits without trailing zeros', () => {
    expect(formatRate(4 * 1024 * 1024)).toBe('4 MB/s');
    expect(formatRate(1.5 * 1024 * 1024)).toBe('1.5 MB/s');
    expect(formatRate(512 * 1024)).toBe('512 KB/s');
  });

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

  it('guesses file names and folders', () => {
    expect(fileNameFromUrl('https://example.com/dir/My%20File.pdf?x=1')).toBe('My File.pdf');
    expect(fileNameFromUrl('https://example.com/')).toBe('example.com');
    expect(folderOf('C:\\Users\\you\\Downloads\\a.zip')).toBe('C:\\Users\\you\\Downloads');
    expect(folderOf('/home/you/a.zip')).toBe('/home/you');
  });
});
