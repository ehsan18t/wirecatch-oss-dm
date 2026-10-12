import { describe, expect, it } from 'vitest';
import {
  GB,
  KB,
  MB,
  bytesAtBitRate,
  formatBytes,
  formatRate,
  formatSpeed,
  inUnit,
  splitKbOrMb,
  splitSize,
  toBytes,
} from './index';
import CASES from './format-bytes.cases.json';

describe('formatBytes', () => {
  // The cases the Rust copy of this function is held to as well.
  it.each(CASES)('shows $bytes bytes as $shown', ({ bytes, shown }) => {
    expect(formatBytes(bytes)).toBe(shown);
  });

  it('shows what is not a size as nothing, and a fraction of a byte rounded', () => {
    expect(formatBytes(-1)).toBe('0 B');
    expect(formatBytes(Number.NaN)).toBe('0 B');
    expect(formatBytes(KB - 0.4)).toBe('1.00 KB');
  });
});

describe('formatSpeed and formatRate', () => {
  it('shows no speed as nothing', () => {
    expect(formatSpeed(0)).toBe('');
    expect(formatSpeed(2 * MB)).toBe('2.00 MB/s');
  });

  it('writes limits without trailing zeros', () => {
    expect(formatRate(4 * MB)).toBe('4 MB/s');
    expect(formatRate(1.5 * MB)).toBe('1.5 MB/s');
    expect(formatRate(512 * KB)).toBe('512 KB/s');
  });
});

describe('size fields', () => {
  it('shows a size in the largest unit that makes it whole', () => {
    expect(splitSize(0)).toEqual({ value: 0, unit: 'MB' });
    expect(splitSize(5 * MB)).toEqual({ value: 5, unit: 'MB' });
    expect(splitSize(2 * GB)).toEqual({ value: 2, unit: 'GB' });
    expect(splitSize(1536)).toEqual({ value: 1.5, unit: 'KB' });
    expect(splitSize(1000)).toEqual({ value: 0.98, unit: 'KB' });
  });

  it('reads what a person typed', () => {
    expect(toBytes('5', 'MB')).toBe(5 * MB);
    expect(toBytes('1,5', 'KB')).toBe(1536);
    expect(toBytes('', 'MB')).toBe(0);
    expect(toBytes('-1', 'MB')).toBeNull();
    expect(toBytes('abc', 'MB')).toBeNull();
  });

  it('reads a number field as it is', () => {
    expect(toBytes(1.5, 'MB')).toBe(1.5 * MB);
    expect(toBytes(Number.NaN, 'KB')).toBeNull();
    expect(toBytes(-2, 'KB')).toBeNull();
  });

  it('shows a size in a given unit to one decimal', () => {
    expect(inUnit(5 * MB, 'MB')).toBe(5);
    expect(inUnit(1.25 * MB, 'MB')).toBe(1.3);
    expect(inUnit(1000, 'KB')).toBe(1);
    expect(inUnit(MB - 1, 'KB')).toBe(1024);
    expect(inUnit(0, 'MB')).toBe(0);
  });

  it('shows a size in megabytes from 1 MB on and in kilobytes below', () => {
    expect(splitKbOrMb(0)).toEqual({ value: 0, unit: 'KB' });
    expect(splitKbOrMb(512 * KB)).toEqual({ value: 512, unit: 'KB' });
    expect(splitKbOrMb(MB)).toEqual({ value: 1, unit: 'MB' });
    expect(splitKbOrMb(1.5 * MB)).toEqual({ value: 1.5, unit: 'MB' });
    expect(splitKbOrMb(3 * GB)).toEqual({ value: 3072, unit: 'MB' });
    // Just under a megabyte stays in kilobytes, rounded up to the next one.
    expect(splitKbOrMb(MB - 1)).toEqual({ value: 1024, unit: 'KB' });
  });
});

describe('bytesAtBitRate', () => {
  it('turns a bit rate and a length into bytes', () => {
    expect(bytesAtBitRate(128, 60_000)).toBe(960_000);
    expect(bytesAtBitRate(0, 60_000)).toBe(0);
  });
});
