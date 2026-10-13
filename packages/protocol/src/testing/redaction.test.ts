import { describe, expect, it } from 'vitest';
import { expected, joined, redactionCases, ruleCases, secretsFor, wrappings } from './redaction';

describe('the redaction samples', () => {
  it('joins pieces into the text they stand for', () => {
    expect(joined(['a', 'b', 'c'])).toBe('abc');
    expect(joined('whole')).toBe('whole');
  });

  it('reads every case as the Rust loader does, each saying what it tests and holding what it names', () => {
    const cases = redactionCases();
    expect(cases.length).toBeGreaterThan(0);
    for (const sample of cases) {
      const tests =
        sample.secrets.length + sample.private.length + sample.keptBy.length > 0 ||
        [sample.log, sample.display, sample.diagnostics].some((exact) => exact !== null);
      expect(tests, sample.text).toBe(true);
      for (const secret of [...sample.secrets, ...sample.private]) {
        expect(sample.text, secret).toContain(secret);
      }
      if (sample.skippedBy !== null) expect(expected(sample, sample.skippedBy)).toBeNull();
      if (sample.keptBy.length > 0) expect(sample.secrets).toEqual([]);
    }
  });

  it('gives a skipped side and the display no secrets to hide', () => {
    const skipped = redactionCases().find((sample) => sample.skippedBy === 'diagnostics');
    expect(skipped).toBeDefined();
    if (skipped === undefined) return;
    expect(secretsFor(skipped, 'diagnostics')).toEqual([]);
    expect(secretsFor(skipped, 'log')).toEqual(skipped.secrets);
    expect(secretsFor(skipped, 'display')).toEqual([]);
  });

  it('has the cases of each rule, and places a text in every wrapping', () => {
    for (const rule of [
      'is_secret_name',
      'names_a_password',
      'looks_like_a_credential',
      'is_service_key',
    ]) {
      expect(ruleCases(rule).length, rule).toBeGreaterThan(0);
    }
    expect(() => ruleCases('no_such_rule')).toThrow();
    const wrapped = wrappings('$&TEXT');
    expect(wrapped.length).toBeGreaterThan(1);
    for (const text of wrapped) expect(text).toContain('$&TEXT');
  });
});
