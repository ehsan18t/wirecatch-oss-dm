import { describe, expect, it } from 'vitest';
import { isSecretName, isServiceKey, looksLikeACredential, namesAPassword } from './secrets';
import { ruleCases } from './testing/redaction';

describe.each([
  ['is_secret_name', isSecretName],
  ['names_a_password', namesAPassword],
  ['looks_like_a_credential', looksLikeACredential],
  ['is_service_key', isServiceKey],
])('%s', (name, rule) => {
  it('answers every case the way the Rust rule answers it', () => {
    const cases = ruleCases(name);
    expect(cases.length).toBeGreaterThan(0);
    const wrong = cases.flatMap(({ input, answer }) =>
      rule(input) === answer ? [] : [`${JSON.stringify(input)} is not answered ${String(answer)}`],
    );
    expect(wrong).toEqual([]);
  });
});
