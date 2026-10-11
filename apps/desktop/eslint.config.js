import { globals, wirecatch } from '@wirecatch/eslint-config';
import svelteConfig from './svelte.config.js';

// Only the backend layer talks to Tauri; everything else goes through `$lib/backend`, so the mock backend can stand in for it.
const NO_TAURI_IMPORT = {
  group: ['@tauri-apps/*'],
  message: 'Call Tauri through `$lib/backend` only.',
};
const NO_TAURI_DYNAMIC_IMPORT = {
  selector: 'ImportExpression[source.value=/^@tauri-apps\\//]',
  message: 'Call Tauri through `$lib/backend` only.',
};

export default wirecatch({
  tsconfigRootDir: import.meta.dirname,
  globals: { ...globals.browser },
  ignores: ['build/', '.svelte-kit/', 'src-tauri/', 'test-results/', 'playwright-report/'],
  svelteConfig,
  layers: [
    {
      files: ['src/**'],
      ignores: ['src/lib/backend/**', 'src/**/*.test.ts', 'src/lib/testing/**'],
      rules: {
        'no-restricted-imports': [
          'error',
          {
            patterns: [
              NO_TAURI_IMPORT,
              {
                group: ['$lib/backend/*', '**/backend/*'],
                message: 'Import the backend from `$lib/backend`, not from one implementation.',
              },
            ],
          },
        ],
        'no-restricted-syntax': ['error', NO_TAURI_DYNAMIC_IMPORT],
      },
    },
    {
      // Tests may use the mock backend directly, and nothing else of the backend or of Tauri.
      files: ['src/**/*.test.ts', 'src/lib/testing/**'],
      ignores: ['src/lib/backend/**'],
      rules: {
        'no-restricted-imports': [
          'error',
          {
            patterns: [
              NO_TAURI_IMPORT,
              {
                group: [
                  '$lib/backend/*',
                  '**/backend/*',
                  '!$lib/backend/mock',
                  '!**/backend/mock',
                  '!$lib/backend/mock-data',
                  '!**/backend/mock-data',
                ],
                message:
                  'Tests use the mock backend (`$lib/backend/mock` and its data), not another implementation.',
              },
            ],
          },
        ],
        'no-restricted-syntax': ['error', NO_TAURI_DYNAMIC_IMPORT],
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
              NO_TAURI_IMPORT,
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
