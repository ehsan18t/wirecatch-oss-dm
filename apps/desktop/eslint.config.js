import { globals, wirecatch } from '@wirecatch/eslint-config';
import svelteConfig from './svelte.config.js';

export default wirecatch({
  tsconfigRootDir: import.meta.dirname,
  globals: { ...globals.browser },
  ignores: ['build/', '.svelte-kit/', 'src-tauri/', 'test-results/', 'playwright-report/'],
  svelteConfig,
  layers: [
    {
      // Only the backend layer talks to Tauri; everything else goes through `$lib/backend`, so the mock backend can stand in for it.
      files: ['src/**'],
      ignores: ['src/lib/backend/**'],
      rules: {
        'no-restricted-imports': [
          'error',
          {
            patterns: [
              { group: ['@tauri-apps/*'], message: 'Call Tauri through `$lib/backend` only.' },
              {
                group: ['$lib/backend/*', '**/backend/*'],
                message: 'Import the backend from `$lib/backend`, not from one implementation.',
              },
            ],
          },
        ],
        'no-restricted-syntax': [
          'error',
          {
            selector: 'ImportExpression[source.value=/^@tauri-apps\\//]',
            message: 'Call Tauri through `$lib/backend` only.',
          },
        ],
      },
    },
    {
      // Components show state and report what the person did; the state classes call the backend, and the routes wire the two together.
      files: ['src/lib/components/**'],
      rules: {
        'no-restricted-imports': [
          'error',
          {
            patterns: [
              { group: ['@tauri-apps/*'], message: 'Call Tauri through `$lib/backend` only.' },
              {
                group: ['$lib/backend', '$lib/backend/*', '**/backend', '**/backend/*'],
                message: 'Components get what they need from state, not from the backend.',
              },
            ],
          },
        ],
      },
    },
  ],
});
