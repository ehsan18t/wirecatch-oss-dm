import type { EasingFunction } from 'svelte/transition';

/** The binomial weight of the two middle control points in a cubic Bézier: B(t) = (1-t)³P0 + 3(1-t)²tP1 + 3(1-t)t²P2 + t³P3. */
const MIDDLE_WEIGHT = 3;
/** Halvings of [0, 1] to find the curve parameter for a given x: 2⁻²⁰ is far below a frame's precision. */
const BISECTIONS = 20;
/** x1, y1, x2, y2. */
const BEZIER_NUMBERS = 4;

/** One coordinate of a cubic Bézier from (0, 0) to (1, 1) with control points `a` and `b`, at parameter `t`. */
function bezier(a: number, b: number, t: number): number {
  const u = 1 - t;
  return MIDDLE_WEIGHT * a * t * u * u + MIDDLE_WEIGHT * b * t * t * u + t * t * t;
}

/** The easing a CSS `cubic-bezier(x1, y1, x2, y2)` describes, for transitions Svelte runs itself. */
export function cubicBezier([x1, y1, x2, y2]: readonly [
  number,
  number,
  number,
  number,
]): EasingFunction {
  return (x) => {
    if (x <= 0) return 0;
    if (x >= 1) return 1;
    let lo = 0;
    let hi = 1;
    for (let i = 0; i < BISECTIONS; i++) {
      const mid = (lo + hi) / 2;
      if (bezier(x1, x2, mid) < x) lo = mid;
      else hi = mid;
    }
    return bezier(y1, y2, (lo + hi) / 2);
  };
}

/** The four numbers of a `cubic-bezier()` token, or null for anything else. */
export function parseCubicBezier(value: string): [number, number, number, number] | null {
  const match = /^cubic-bezier\(([^)]*)\)$/.exec(value.trim());
  if (!match?.[1]) return null;
  const numbers = match[1].split(',').map(Number);
  const [x1, y1, x2, y2] = numbers;
  if (numbers.length !== BEZIER_NUMBERS || numbers.some((n) => !Number.isFinite(n))) return null;
  if (x1 === undefined || y1 === undefined || x2 === undefined || y2 === undefined) return null;
  return [x1, y1, x2, y2];
}
