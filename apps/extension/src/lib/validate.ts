/**
 * Reading values whose shape is not known: messages from the popup, content scripts
 * and the bridge, and what browser storage gives back. Each check is a type guard,
 * and the combinators build guards for objects, lists and unions out of smaller
 * ones, so a shape is written once, as data, instead of as a chain of `typeof` tests.
 *
 * Objects may carry fields the guard does not name; they are not checked.
 */

declare const exactly: unique symbol;
declare const loose: unique symbol;

/**
 * True when `value` has the shape `T`. The phantom field makes `Guard<T>` invariant in `T`, so a
 * guard for a narrower or wider type than the one asked for does not compile; a plain type
 * predicate would be accepted for any wider type.
 */
export type Guard<T> = ((value: unknown) => value is T) & {
  readonly [exactly]?: (value: T) => T;
  readonly [loose]?: never;
};

/**
 * A guard built on `object`, which `Guard<T>` does not accept: two object types that differ only
 * in an optional field accept each other, so such a guard cannot be held to a named type. The
 * wrappers keep the mark, so wrapping one in `nullable`, `arrayOf` and the rest does not hide it.
 */
export type LooseGuard<T> = ((value: unknown) => value is T) & {
  readonly [exactly]?: (value: T) => T;
  readonly [loose]: true;
};

/** Any guard, whatever it checks for, where combinators take guards of differing types. */
type Check = (value: unknown) => boolean;

/** The type a guard checks for. */
export type Guarded<G> = G extends (value: unknown) => value is infer T ? T : never;

type Shape = Record<string, Check>;

/** Keys whose guard accepts `undefined`, which an object may therefore leave out. */
type OptionalKeys<S extends Shape> = {
  [K in keyof S]: undefined extends Guarded<S[K]> ? K : never;
}[keyof S];

type ObjectOf<S extends Shape> = {
  [K in Exclude<keyof S, OptionalKeys<S>>]: Guarded<S[K]>;
} & { [K in OptionalKeys<S>]?: Guarded<S[K]> };

/** Values a `literal` guard compares against. */
type Literal = string | number | boolean | null;

export const isRecord: Guard<Record<string, unknown>> = (value): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null;

export const isString: Guard<string> = (value): value is string => typeof value === 'string';
export const isNumber: Guard<number> = (value): value is number => typeof value === 'number';
export const isBoolean: Guard<boolean> = (value): value is boolean => typeof value === 'boolean';
/** Accepts anything, for a field whose value is read leniently after the check. */
export const isUnknown: Guard<unknown> = (_value): _value is unknown => true;

/**
 * One of the given values, compared with `===`. Typed as a guard of a wider union, it may
 * name fewer members than the union has without the compiler noticing: use it for a single
 * tag, and `member` for a union a type defines.
 */
export function literal<const T extends readonly Literal[]>(...values: T): Guard<T[number]> {
  return (value): value is T[number] => values.some((v) => v === value);
}

export function nullable<T>(guard: LooseGuard<T>): LooseGuard<T | null>;
export function nullable<T>(guard: Guard<T>): Guard<T | null>;
export function nullable<T>(
  guard: Guard<T> | LooseGuard<T>,
): Guard<T | null> | LooseGuard<T | null> {
  return (value): value is T | null => value === null || guard(value);
}

/** The value may be missing (`undefined`); as an object field, the key may be left out. */
export function optional<T>(guard: LooseGuard<T>): LooseGuard<T | undefined>;
export function optional<T>(guard: Guard<T>): Guard<T | undefined>;
export function optional<T>(
  guard: Guard<T> | LooseGuard<T>,
): Guard<T | undefined> | LooseGuard<T | undefined> {
  return (value): value is T | undefined => value === undefined || guard(value);
}

/** Narrows a guard further, for limits a type cannot express (a length, a range). */
export function refine<T>(guard: LooseGuard<T>, test: (value: T) => boolean): LooseGuard<T>;
export function refine<T>(guard: Guard<T>, test: (value: T) => boolean): Guard<T>;
export function refine<T>(
  guard: Guard<T> | LooseGuard<T>,
  test: (value: T) => boolean,
): Guard<T> | LooseGuard<T> {
  return (value): value is T => guard(value) && test(value);
}

/** An array whose every item passes `guard`. */
export function arrayOf<T>(guard: LooseGuard<T>): LooseGuard<T[]>;
export function arrayOf<T>(guard: Guard<T>): Guard<T[]>;
export function arrayOf<T>(guard: Guard<T> | LooseGuard<T>): Guard<T[]> | LooseGuard<T[]> {
  return (value): value is T[] => Array.isArray(value) && value.every(guard);
}

/** A guard of `T`, loose when any of `G` is. */
type LooseIfAny<G extends readonly Check[], T> = G[number] extends { readonly [loose]?: never }
  ? Guard<T>
  : LooseGuard<T>;

/** An array whose first items pass the given guards in order; later items are not checked. */
export function tuple<const G extends readonly Check[]>(
  ...guards: G
): LooseIfAny<G, { [I in keyof G]: Guarded<G[I]> }>;
export function tuple<const G extends readonly Check[]>(
  ...guards: G
): Guard<{ [I in keyof G]: Guarded<G[I]> }> {
  return (value): value is { [I in keyof G]: Guarded<G[I]> } =>
    Array.isArray(value) && guards.every((guard, i) => guard(value[i]));
}

/**
 * An object whose named fields pass their guards, for a shape no type names (an envelope, a
 * lenient read). For a named type use `fields`, which also catches a missing optional field.
 */
export function object<S extends Shape>(shape: S): LooseGuard<ObjectOf<S>> {
  const fields = Object.entries(shape);
  const check = (value: unknown): value is ObjectOf<S> =>
    isRecord(value) && fields.every(([key, guard]) => guard(value[key]));
  // The mark exists only in the type; this is the one place a guard gets it.
  return check as LooseGuard<ObjectOf<S>>;
}

/** A value that passes any one of the guards: a union, usually told apart by a `type` field. */
export function oneOf<const G extends readonly Check[]>(
  ...guards: G
): LooseIfAny<G, Guarded<G[number]>>;
export function oneOf<const G extends readonly Check[]>(...guards: G): Guard<Guarded<G[number]>> {
  return (value): value is Guarded<G[number]> => guards.some((guard) => guard(value));
}

/**
 * One member of the string union `T`. The members are given as the keys of a record
 * typed `Record<T, true>`, so the compiler rejects a list that misses a member or names
 * one that does not exist: the guard cannot fall behind the type.
 */
/** The members of `T` as `member` takes them; nothing fits until `T` is given, as it cannot be inferred. */
type Members<T extends string> = [T] extends [never] ? never : Record<NoInfer<T>, true>;

export function member<T extends string = never>(members: Members<T>): Guard<T> {
  const known = new Set<string>(Object.keys(members));
  return (value): value is T => typeof value === 'string' && known.has(value);
}

/**
 * An object of type `T`, with a guard for every one of its fields, optional ones included
 * (their guard accepts `undefined`), so the compiler rejects a shape that leaves a field out.
 */
export function fields<T extends object>(shape: {
  [K in keyof T]-?: Guard<T[K]>;
}): Guard<T> {
  const entries: [string, Check][] = Object.entries<Check>(shape);
  return (value): value is T =>
    isRecord(value) && entries.every(([key, guard]) => guard(value[key]));
}

/** The member of the union `U` whose tag (the `K` field) is `T`, for building its guard with `fields`. */
export type Variant<U, T, K extends string = 'type'> = Extract<U, Record<K, T>>;

/** Values a union member can be told apart by. */
type Tag = string | boolean;

/** One guard per member of the union `U`, keyed by the member's tag (the `K` field) as text. */
type VariantGuards<U extends Record<K, Tag>, K extends string> = {
  [T in U[K] as `${T}`]: Guard<Extract<U, Record<K, T>>>;
};

/**
 * A union told apart by its tag field (`type` unless `K` says otherwise), with one guard
 * per member, keyed by the tag (`true` and `false` for a boolean one). The record must name
 * every member, so the guard cannot fall behind the type; each member's guard checks the
 * tag itself.
 */
export function variants<U extends Record<K, Tag>, K extends string = 'type'>(
  guards: VariantGuards<U, K>,
): Guard<U> {
  const list: Check[] = Object.values<Check>(guards);
  return (value): value is U => list.some((guard) => guard(value));
}

/** The items of `value` that pass `guard`; none when it is not an array. Drops what it cannot read. */
export function itemsOf<T>(value: unknown, guard: Guard<T>): T[] {
  return Array.isArray(value) ? value.filter(guard) : [];
}
