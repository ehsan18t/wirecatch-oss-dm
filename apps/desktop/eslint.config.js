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
                  '$lib/backend/mock/*',
                  '**/backend/mock/*',
                ],
                message:
                  'Tests use the mock backend (`$lib/backend/mock`, which also exports its sample data), not another implementation or its parts.',
              },
            ],
          },
        ],
        'no-restricted-syntax': ['error', NO_TAURI_DYNAMIC_IMPORT],
      },
    },
    {
      // Buttons and icons go through Button, IconButton and Icon, so their styles and sizes have one home.
      files: ['src/**/*.svelte'],
      ignores: [
        'src/lib/components/Button.svelte',
        'src/lib/components/IconButton.svelte',
        'src/lib/components/Icon.svelte',
      ],
      rules: {
        'wirecatch/use-ui-primitives': [
          'error',
          { buttonClasses: { btn: 'Button', 'icon-btn': 'IconButton' }, icon: 'Icon' },
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
