import { after, describe, it } from 'node:test';
import { RuleTester } from 'eslint';
import sveltePlugin from 'eslint-plugin-svelte';
import { noProductName } from './no-product-name.js';

RuleTester.describe = describe;
RuleTester.it = it;
RuleTester.afterAll = after;

const svelteParser = sveltePlugin.configs.base.find((c) => c.languageOptions?.parser)
  ?.languageOptions.parser;

const options = [{ name: 'Acme' }];
const errors = [{ messageId: 'written' }];

const scripts = new RuleTester({
  languageOptions: { ecmaVersion: 'latest', sourceType: 'module' },
});

scripts.run('no-product-name', noProductName, {
  valid: [
    { code: '// Acme says so\nconst a = 1;', options },
    { code: 'const Acme = NAME;', options },
    { code: "const a = 'acme-storage-key';", options },
    { code: "const a = 'AcmeNotices';", options },
    { code: 'const a = `${NAME} stopped`;', options },
  ],
  invalid: [
    { code: "const a = 'Acme stopped';", options, errors },
    { code: 'const a = "Open Acme.";', options, errors },
    { code: 'const a = `Sent to Acme: ${b}`;', options, errors },
  ],
});

const svelte = new RuleTester({ languageOptions: { parser: svelteParser } });

svelte.run('no-product-name in Svelte', noProductName, {
  valid: [
    { code: '<!-- Acme --><p>{name} stopped</p>', filename: 'a.svelte', options },
    { code: '<Button label={name} />', filename: 'a.svelte', options },
  ],
  invalid: [
    { code: '<p>Connect Acme</p>', filename: 'a.svelte', options, errors },
    { code: '<Button label="Open Acme" />', filename: 'a.svelte', options, errors },
    { code: '<Button label={`Open Acme`} />', filename: 'a.svelte', options, errors },
  ],
});
