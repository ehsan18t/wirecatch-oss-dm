import { describe, expect, it } from 'vitest';
import { DEFAULT_WIDTH, gridColumns, shownColumns } from './columns';

/** jsdom's root text size. */
const REM = 16;

describe('the list columns', () => {
  it('shows every number column where there is room, and all of them where nothing is laid out', () => {
    expect([...shownColumns(0, {})]).toEqual(['progress', 'size', 'speed', 'left']);
    expect([...shownColumns(62 * REM, {})]).toEqual(['progress', 'size', 'speed', 'left']);
  });

  it('gives way Time left first, then Speed, Size and progress, keeping the name its room', () => {
    expect([...shownColumns(57 * REM, {})]).toEqual(['progress', 'size', 'speed']);
    expect([...shownColumns(51 * REM, {})]).toEqual(['progress', 'size']);
    expect([...shownColumns(43 * REM, {})]).toEqual(['progress']);
    expect([...shownColumns(30 * REM, {})]).toEqual([]);
  });

  it('counts a widened column at its width, so the name is never squeezed out', () => {
    const wide = { progress: DEFAULT_WIDTH.progress * 3 * REM };
    expect([...shownColumns(71 * REM, wide)]).toEqual(['progress', 'size']);
    expect([...shownColumns(62 * REM, wide)]).toEqual([]);
  });

  it('lays out the grid with the widths the person set', () => {
    expect(gridColumns(new Set(['progress', 'left']), { left: 90 })).toBe(
      '2rem 2.75rem minmax(0, 1fr) 10.5rem 90px 4.75rem',
    );
  });
});
