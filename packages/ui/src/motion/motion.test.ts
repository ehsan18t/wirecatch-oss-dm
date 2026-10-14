import { afterEach, describe, expect, it } from 'vitest';
import { setReducedMotion } from '../testing/setup';
import { cubicBezier, parseCubicBezier } from './easing';
import { delay, duration, fade, shift } from './motion';

const html = document.documentElement;

describe('motion', () => {
  afterEach(() => {
    html.style.removeProperty('--transition-duration-slow');
    html.style.removeProperty('--transition-delay-tooltip');
  });

  // The token cache keeps the first value it reads, so every test sets the same values.
  function setTokens() {
    html.style.setProperty('--transition-duration-slow', '200ms');
    html.style.setProperty('--transition-delay-tooltip', '0.4s');
  }

  it('reads durations and delays from the tokens', () => {
    setTokens();
    expect(duration('slow')).toBe(200);
    expect(delay('tooltip')).toBe(400);
    expect(fade(html, { speed: 'slow' }).duration).toBe(200);
  });

  it('runs no Svelte motion under reduced motion, but keeps intent delays', () => {
    setTokens();
    setReducedMotion(true);
    expect(duration('slow')).toBe(0);
    expect(shift(html).duration).toBe(0);
    expect(delay('tooltip')).toBe(400);
  });
});

describe('easing', () => {
  it('reads cubic-bezier tokens and refuses anything else', () => {
    expect(parseCubicBezier('cubic-bezier(0, 0, 0, 1)')).toEqual([0, 0, 0, 1]);
    expect(parseCubicBezier('ease-out')).toBeNull();
    expect(parseCubicBezier('cubic-bezier(0, 0, 1)')).toBeNull();
  });

  it('follows the curve from 0 to 1', () => {
    const linear = cubicBezier([0, 0, 1, 1]);
    expect(linear(0.5)).toBeCloseTo(0.5, 3);
    const out = cubicBezier([0, 0, 0, 1]);
    expect(out(0.25)).toBeGreaterThan(0.25);
    expect(out(0)).toBe(0);
    expect(out(1)).toBe(1);
  });
});
