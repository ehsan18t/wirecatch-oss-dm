import { describe, expect, it } from 'vitest';
import {
  arrayOf,
  fields,
  type Guard,
  isBoolean,
  isNumber,
  isRecord,
  isString,
  isUnknown,
  itemsOf,
  literal,
  member,
  nullable,
  object,
  oneOf,
  optional,
  refine,
  tuple,
  type Variant,
  variants,
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

  it('accepts only the listed members of a string union', () => {
    const isSize = member<'s' | 'm'>({ s: true, m: true });
    expect(isSize('s')).toBe(true);
    expect(isSize('l')).toBe(false);
    expect(isSize('toString')).toBe(false);
  });

  it('tells a union apart by its tag, with each member checked whole', () => {
    type Shape = { kind: 'dot' } | { kind: 'line'; length: number };
    const isShape = variants<Shape, 'kind'>({
      dot: fields<Variant<Shape, 'dot', 'kind'>>({ kind: literal('dot') }),
      line: fields<Variant<Shape, 'line', 'kind'>>({ kind: literal('line'), length: isNumber }),
    });
    expect(isShape({ kind: 'dot' })).toBe(true);
    expect(isShape({ kind: 'line', length: 2 })).toBe(true);
    expect(isShape({ kind: 'line' })).toBe(false);
    expect(isShape({ kind: 'circle' })).toBe(false);
  });

  it('checks every field of a type, optional ones included', () => {
    interface Entry {
      name: string;
      size?: number;
    }
    const isEntry = fields<Entry>({ name: isString, size: optional(isNumber) });
    expect(isEntry({ name: 'a' })).toBe(true);
    expect(isEntry({ name: 'a', size: 2 })).toBe(true);
    expect(isEntry({ name: 'a', size: '2' })).toBe(false);
  });

  it('tells a union apart by a boolean tag', () => {
    type Answer = { ok: true; value: number } | { ok: false; reason: string };
    const isAnswer = variants<Answer, 'ok'>({
      true: fields<Variant<Answer, true, 'ok'>>({ ok: literal(true), value: isNumber }),
      false: fields<Variant<Answer, false, 'ok'>>({ ok: literal(false), reason: isString }),
    });
    expect(isAnswer({ ok: true, value: 1 })).toBe(true);
    expect(isAnswer({ ok: false, reason: 'no' })).toBe(true);
    expect(isAnswer({ ok: true, reason: 'no' })).toBe(false);
  });

  // These guards promise that a guard cannot fall behind its type; the compiler checks the
  // lines below, which must stay errors.
  it('makes the compiler reject a guard that leaves out a field, member or variant', () => {
    interface Entry {
      name: string;
      size?: number;
    }
    // @ts-expect-error -- the optional field `size` has no guard.
    fields<Entry>({ name: isString });
    // @ts-expect-error -- the member 'm' is missing.
    member<'s' | 'm'>({ s: true });
    type Shape = { type: 'dot' } | { type: 'line'; length: number };
    // @ts-expect-error -- the variant 'line' is missing.
    variants<Shape>({ dot: fields<Variant<Shape, 'dot'>>({ type: literal('dot') }) });
    expect(true).toBe(true);
  });

  it('makes the compiler reject a guard narrower or wider than its type', () => {
    interface Limit {
      size: number | null;
      note?: string;
    }
    // @ts-expect-error -- `size` may be null, which `isNumber` rejects.
    fields<Limit>({ size: isNumber, note: optional(isString) });
    // @ts-expect-error -- `note` is optional, but its guard rejects a missing value.
    fields<Limit>({ size: nullable(isNumber), note: isString });
    interface Wrapper {
      type: 'a';
      data: Limit;
    }
    // @ts-expect-error -- `object` cannot tell that `note`, an optional field, is left out.
    fields<Wrapper>({ type: literal('a'), data: object({ size: nullable(isNumber) }) });
    // @ts-expect-error -- the shape leaves out `note`, an optional field of `Limit`.
    fields<Wrapper>({ type: literal('a'), data: fields<Limit>({ size: nullable(isNumber) }) });
    // @ts-expect-error -- `nullable` keeps the mark of the `object` guard it wraps.
    fields<{ data: Limit | null }>({ data: nullable(object({ size: nullable(isNumber) })) });
    // @ts-expect-error -- `arrayOf` keeps the mark of the `object` guard it wraps.
    fields<{ data: Limit[] }>({ data: arrayOf(object({ size: nullable(isNumber) })) });
    // @ts-expect-error -- `oneOf` is loose when any guard it takes is.
    fields<{ data: Limit }>({ data: oneOf(object({ size: nullable(isNumber) })) });
    // @ts-expect-error -- an `object` guard is not a `Guard` of a named type.
    const asNamed: Guard<Limit> = object({ size: nullable(isNumber) });
    // @ts-expect-error -- `v` is never null, which `nullable(isNumber)` allows.
    fields<{ v: number }>({ v: nullable(isNumber) });
    // @ts-expect-error -- without its type, `member` cannot know every member.
    member({ s: true });
    expect(asNamed).toBeTypeOf('function');
  });

  it('keeps the items that pass and drops the rest', () => {
    expect(itemsOf([1, 'a', 2], isNumber)).toEqual([1, 2]);
    expect(itemsOf('not a list', isNumber)).toEqual([]);
  });
});
