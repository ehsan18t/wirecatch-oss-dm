import { describe, expect, it } from 'vitest';
import { FIRST_STRONG_ISOLATE, isolate, ordinal, POP_DIRECTIONAL_ISOLATE } from './text';

describe('text', () => {
  it('writes English ordinals', () => {
    const cases: [number, string][] = [
      [1, '1st'],
      [2, '2nd'],
      [3, '3rd'],
      [4, '4th'],
      [11, '11th'],
      [12, '12th'],
      [13, '13th'],
      [21, '21st'],
      [22, '22nd'],
      [101, '101st'],
      [111, '111th'],
    ];
    for (const [n, text] of cases) expect(ordinal(n)).toBe(text);
  });

  it('isolates a name with U+2068 and U+2069', () => {
    expect(FIRST_STRONG_ISOLATE.codePointAt(0)).toBe(0x2068);
    expect(POP_DIRECTIONAL_ISOLATE.codePointAt(0)).toBe(0x2069);
    expect(isolate('a.exe')).toBe(`${FIRST_STRONG_ISOLATE}a.exe${POP_DIRECTIONAL_ISOLATE}`);
  });
});
