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
      },
    },
  ],
});
