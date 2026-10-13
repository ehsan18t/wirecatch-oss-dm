import { globals, wirecatch } from '@wirecatch/eslint-config';
import svelteConfig from './svelte.config.js';

const NATIVE_ONLY =
  'Talk to the app through `src/lib/native` only: `port.ts` opens the connection and `client.ts` uses it.';

export default wirecatch({
  tsconfigRootDir: import.meta.dirname,
  globals: { ...globals.browser, ...globals.webextensions },
  ignores: ['.output/', '.wxt/'],
  svelteConfig,
  layers: [
    {
      // Every caught error is logged, used or explained in place (docs/ENGINEERING.md, section 6);
      // `fallback` from the logger makes a handler that logs.
      files: ['src/**'],
      rules: {
        'wirecatch/no-silent-catch': [
          'error',
          { factories: [{ name: 'fallback', module: 'diag/log' }] },
        ],
      },
    },
    {
      // Only `src/lib/native` opens the connection to the app, so there is one place that owns its lifecycle and errors.
      files: ['src/**'],
      ignores: ['src/lib/native/**'],
      rules: {
        'no-restricted-syntax': [
          'error',
          {
            selector: 'MemberExpression[property.name=/^(connectNative|sendNativeMessage)$/]',
            message: NATIVE_ONLY,
          },
          {
            selector: 'ObjectPattern > Property[key.name=/^(connectNative|sendNativeMessage)$/]',
            message: NATIVE_ONLY,
          },
        ],
      },
    },
  ],
});
