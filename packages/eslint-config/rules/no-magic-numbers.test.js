import { after, describe, it } from 'node:test';
import { RuleTester } from 'eslint';
import ts from 'typescript-eslint';
import { noMagicNumbers } from './no-magic-numbers.js';

RuleTester.describe = describe;
RuleTester.it = it;
RuleTester.afterAll = after;

const tester = new RuleTester({ languageOptions: { parser: ts.parser } });
const options = [{ ignore: [0, 1] }];
const magic = [{ messageId: 'noMagic' }];

tester.run('no-magic-numbers', noMagicNumbers, {
  valid: [
    { code: 'const MAX_URL_LENGTH = 8 * 1024;', options },
    { code: 'export const BACKOFF_MS = [1_000, 2_000, 4_000] as const;', options },
    { code: 'const OFFSET = -42;', options },
    {
      code: 'const LIMITS = { files: 50, bytes: 4 * 1024 * 1024 } satisfies Record<string, number>;',
      options,
    },
    { code: 'const STEPS = [[5, 10], [15, 20]];', options },
  ],
  invalid: [
    // Inside a function the constant names something local; the base rule decides.
    {
      code: 'function f() { const digits = 3 * 2; return digits; }',
      options,
      errors: [...magic, ...magic],
    },
    // A call is logic, not a value written out.
    { code: 'const DELAY = Math.max(250, 100);', options, errors: [...magic, ...magic] },
    // `let` can change, so it does not name a value.
    { code: 'let size = 8 * 1024;', options, errors: [...magic, ...magic] },
    { code: 'if (x > 42) {}', options, errors: magic },
    // A computed key is not part of the value.
    { code: 'const TABLE = { [40 + 2]: 0 };', options, errors: [...magic, ...magic] },
  ],
});
