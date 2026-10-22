import { describe, expect, it } from 'vitest';
import { committedValue, keyValue, parseNumber, PAGE_STEPS, stepValue } from './number-input';

const range = { min: 0, max: 10, step: 1 };

describe('number input', () => {
  it('steps within its range', () => {
    expect(stepValue(5, 1, range)).toBe(6);
    expect(stepValue(10, 1, range)).toBe(10);
    expect(stepValue(null, 1, range)).toBe(1);
    expect(stepValue(0.1, 2, { step: 0.1 })).toBe(0.3);
  });

  it('takes the spin button keys', () => {
    expect(keyValue('ArrowUp', 3, range)).toBe(4);
    expect(keyValue('ArrowDown', 3, range)).toBe(2);
    expect(keyValue('PageUp', 0, { step: 1 })).toBe(PAGE_STEPS);
    expect(keyValue('Home', 3, range)).toBe(0);
    expect(keyValue('End', 3, range)).toBe(10);
    expect(keyValue('a', 3, range)).toBeNull();
  });

  it('keeps what is typed to the step: whole numbers for a whole step, within range', () => {
    expect(committedValue(1.2, { min: 1, max: 32, step: 1 })).toBe(1);
    expect(committedValue(2.6, { min: 1, max: 32, step: 1 })).toBe(3);
    expect(committedValue(99, { min: 1, max: 32, step: 1 })).toBe(32);
    expect(committedValue(2.37, { min: 0.1, step: 0.5 })).toBe(2.4);
  });

  it('reads typed numbers with either decimal mark', () => {
    expect(parseNumber(' 12 ')).toBe(12);
    expect(parseNumber('1,5')).toBe(1.5);
    expect(parseNumber('')).toBeNull();
    expect(parseNumber('12a')).toBeNaN();
  });
});
