import { describe, expect, it } from 'vitest';
import { GB, KB, MB, formatBytes, formatRate, formatSpeed, splitSize, toBytes } from './index';

describe('formatBytes', () => {
  it('uses short units with three significant digits at most', () => {
    expect(formatBytes(0)).toBe('0 B');
    expect(formatBytes(512)).toBe('512 B');
    expect(formatBytes(1536)).toBe('1.50 KB');
    expect(formatBytes(15 * MB)).toBe('15.0 MB');
    expect(formatBytes(820 * MB)).toBe('820 MB');
    expect(formatBytes(6.1 * GB)).toBe('6.10 GB');
    expect(formatBytes(-1)).toBe('0 B');
    expect(formatBytes(Number.NaN)).toBe('0 B');
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
});
