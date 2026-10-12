// A small deterministic random source, so tests and screenshots of the sample data repeat exactly.

/** A random number from 0 (inclusive) to 1 (exclusive). */
export type Random = () => number;

/**
 * The mulberry32 generator for `seed`. Its constants are the published algorithm's own: they have no meaning a name could carry, and changing one changes every sample download.
 */
export function mulberry32(seed: number): Random {
  /* eslint-disable wirecatch/no-magic-numbers -- the constants of the published mulberry32 generator (docs/AUDIT.md W14 E1) */
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
  /* eslint-enable wirecatch/no-magic-numbers */
}

/** A range a random value is drawn from: `min` up to `min + span`. */
export interface Spread {
  min: number;
  span: number;
}

/** A value drawn evenly from `spread`. */
export function between(random: Random, spread: Spread): number {
  return spread.min + random() * spread.span;
}
