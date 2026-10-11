import { globals, wirecatch } from '@wirecatch/eslint-config';
import svelteConfig from './svelte.config.js';

const NATIVE_ONLY = 'Talk to the app through `src/lib/native/client.ts` only.';

export default wirecatch({
  tsconfigRootDir: import.meta.dirname,
  globals: { ...globals.browser, ...globals.webextensions },
  ignores: ['.output/', '.wxt/'],
  svelteConfig,
  layers: [
    {
      // Only the native client opens the connection to the app, so there is one place that owns its lifecycle and errors.
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
