/**
 * Reading values whose shape is not known: messages from the popup, content scripts
 * and the bridge, and what browser storage gives back. Each check is a type guard,
 * and the combinators build guards for objects, lists and unions out of smaller
 * ones, so a shape is written once, as data, instead of as a chain of `typeof` tests.
 *
 * Objects may carry fields the guard does not name; they are not checked.
 */

/** True when `value` has the shape `T`. */
export type Guard<T> = (value: unknown) => value is T;

/** The type a guard checks for. */
export type Guarded<G> = G extends Guard<infer T> ? T : never;

type Shape = Record<string, Guard<unknown>>;

/** Keys whose guard accepts `undefined`, which an object may therefore leave out. */
type OptionalKeys<S extends Shape> = {
  [K in keyof S]: undefined extends Guarded<S[K]> ? K : never;
}[keyof S];

type ObjectOf<S extends Shape> = {
  [K in Exclude<keyof S, OptionalKeys<S>>]: Guarded<S[K]>;
} & { [K in OptionalKeys<S>]?: Guarded<S[K]> };

/** Values a `literal` guard compares against. */
type Literal = string | number | boolean | null;

export function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}

export const isString = (value: unknown): value is string => typeof value === 'string';
export const isNumber = (value: unknown): value is number => typeof value === 'number';
export const isBoolean = (value: unknown): value is boolean => typeof value === 'boolean';
/** Accepts anything, for a field whose value is read leniently after the check. */
export const isUnknown = (_value: unknown): _value is unknown => true;

/** One of the given values, compared with `===`. */
export function literal<const T extends readonly Literal[]>(...values: T): Guard<T[number]> {
  return (value): value is T[number] => values.some((v) => v === value);
}

export function nullable<T>(guard: Guard<T>): Guard<T | null> {
  return (value): value is T | null => value === null || guard(value);
}

/** The value may be missing (`undefined`); as an object field, the key may be left out. */
export function optional<T>(guard: Guard<T>): Guard<T | undefined> {
  return (value): value is T | undefined => value === undefined || guard(value);
}

/** Narrows a guard further, for limits a type cannot express (a length, a range). */
export function refine<T>(guard: Guard<T>, test: (value: T) => boolean): Guard<T> {
  return (value): value is T => guard(value) && test(value);
}

/** An array whose every item passes `guard`. */
export function arrayOf<T>(guard: Guard<T>): Guard<T[]> {
  return (value): value is T[] => Array.isArray(value) && value.every(guard);
}

/** An array whose first items pass the given guards in order; later items are not checked. */
export function tuple<const G extends readonly Guard<unknown>[]>(
  ...guards: G
): Guard<{ [I in keyof G]: Guarded<G[I]> }> {
  return (value): value is { [I in keyof G]: Guarded<G[I]> } =>
    Array.isArray(value) && guards.every((guard, i) => guard(value[i]));
}

/** An object whose named fields pass their guards. */
export function object<S extends Shape>(shape: S): Guard<ObjectOf<S>> {
  const fields = Object.entries(shape);
  return (value): value is ObjectOf<S> =>
    isRecord(value) && fields.every(([key, guard]) => guard(value[key]));
}

/** A value that passes any one of the guards: a union, usually told apart by a `type` field. */
export function oneOf<const G extends readonly Guard<unknown>[]>(
  ...guards: G
): Guard<Guarded<G[number]>> {
  return (value): value is Guarded<G[number]> => guards.some((guard) => guard(value));
}

/** The items of `value` that pass `guard`; none when it is not an array. Drops what it cannot read. */
export function itemsOf<T>(value: unknown, guard: Guard<T>): T[] {
  return Array.isArray(value) ? value.filter(guard) : [];
}
