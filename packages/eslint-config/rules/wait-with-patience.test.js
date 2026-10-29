import { after, describe, it } from 'node:test';
import { RuleTester } from 'eslint';
import { waitWithPatience } from './wait-with-patience.js';

RuleTester.describe = describe;
RuleTester.it = it;
RuleTester.afterAll = after;

const tester = new RuleTester({ languageOptions: { ecmaVersion: 'latest', sourceType: 'module' } });

tester.run('wait-with-patience', waitWithPatience, {
  valid: [
    'await vi.waitFor(() => check(), PATIENCE_MS);',
    'await vi.waitFor(() => check(), { timeout: PATIENCE_MS });',
    'await waitFor(() => check());',
  ],
  invalid: [
    { code: 'await vi.waitFor(() => check());', errors: [{ messageId: 'bare' }] },
    { code: 'await vi.waitFor(async () => { await check(); });', errors: [{ messageId: 'bare' }] },
  ],
});
