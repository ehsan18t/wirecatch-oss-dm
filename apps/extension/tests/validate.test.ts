import { describe, expect, it } from 'vitest';
import {
  arrayOf,
  isBoolean,
  isNumber,
  isRecord,
  isString,
  isUnknown,
  itemsOf,
  literal,
  nullable,
  object,
  oneOf,
  optional,
  refine,
  tuple,
} from '../src/lib/validate';

describe('validate', () => {
  it('checks primitive types', () => {
    expect(isString('a')).toBe(true);
    expect(isString(1)).toBe(false);
    expect(isNumber(1)).toBe(true);
    expect(isNumber('1')).toBe(false);
    expect(isBoolean(false)).toBe(true);
    expect(isBoolean(0)).toBe(false);
    expect(isUnknown(undefined)).toBe(true);
    expect(isRecord({})).toBe(true);
    expect(isRecord([])).toBe(true);
    expect(isRecord(null)).toBe(false);
  });

  it('accepts only the listed literals', () => {
    const kind = literal('hls', 'dash');
    expect(kind('hls')).toBe(true);
    expect(kind('direct')).toBe(false);
    expect(literal(1)(1)).toBe(true);
    expect(literal(1)('1')).toBe(false);
  });

  it('adds null, undefined and further limits', () => {
    expect(nullable(isString)(null)).toBe(true);
    expect(nullable(isString)(undefined)).toBe(false);
    expect(optional(isString)(undefined)).toBe(true);
    expect(optional(isString)(null)).toBe(false);
    const short = refine(isString, (s) => s.length <= 2);
    expect(short('ab')).toBe(true);
    expect(short('abc')).toBe(false);
    expect(short(1)).toBe(false);
  });

  it('checks every item of an array, and the first items of a tuple', () => {
    expect(arrayOf(isNumber)([1, 2])).toBe(true);
    expect(arrayOf(isNumber)([1, '2'])).toBe(false);
    expect(arrayOf(isNumber)('12')).toBe(false);
    const entry = tuple(isNumber, optional(isString));
    expect(entry([1])).toBe(true);
    expect(entry([1, 'a', 'later items are not checked'])).toBe(true);
    expect(entry([1, 2])).toBe(false);
    expect(entry({ 0: 1 })).toBe(false);
  });

  it('checks the named fields of an object and lets optional ones be left out', () => {
    const point = object({ x: isNumber, label: optional(isString) });
    expect(point({ x: 1 })).toBe(true);
    expect(point({ x: 1, label: 'a', other: true })).toBe(true);
    expect(point({ x: 1, label: 2 })).toBe(false);
    expect(point({ label: 'a' })).toBe(false);
    expect(point(null)).toBe(false);
  });

  it('accepts any one of several shapes', () => {
    const message = oneOf(
      object({ type: literal('a'), n: isNumber }),
      object({ type: literal('b'), s: isString }),
    );
    expect(message({ type: 'a', n: 1 })).toBe(true);
    expect(message({ type: 'b', s: 's' })).toBe(true);
    expect(message({ type: 'a', s: 's' })).toBe(false);
  });

  it('keeps the items that pass and drops the rest', () => {
    expect(itemsOf([1, 'a', 2], isNumber)).toEqual([1, 2]);
    expect(itemsOf('not a list', isNumber)).toEqual([]);
  });
});
