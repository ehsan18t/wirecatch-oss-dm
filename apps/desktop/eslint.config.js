import { globals, wirecatch } from '@wirecatch/eslint-config';
import svelteConfig from './svelte.config.js';

// Only the backend layer talks to Tauri; everything else goes through `$lib/backend`, so the mock backend can stand in for it.
const NO_TAURI_IMPORT = {
  group: ['@tauri-apps/*'],
  message: 'Call Tauri through `$lib/backend` only.',
};
// The component gallery is dev-only (vite.config.ts fails any build that bundles it); only its own files import it, by relative paths that never name the folder.
const NO_GALLERY_IMPORT = {
  group: ['**/gallery', '**/gallery/**'],
  message: 'The component gallery is dev-only: import it from nowhere outside src/gallery.',
};
const NO_TAURI_DYNAMIC_IMPORT = {
  selector: 'ImportExpression[source.value=/^@tauri-apps\\//]',
  message: 'Call Tauri through `$lib/backend` only.',
};

export default wirecatch({
  tsconfigRootDir: import.meta.dirname,
  globals: { ...globals.browser },
  ignores: ['build/', 'build-gallery/', 'src-tauri/', 'test-results/', 'playwright-report/'],
  svelteConfig,
  // Redesigned screens live in src/ui. The gallery's components are held to the tokens too; its sample data (sample.ts) stands in for what the OS and the backend send, such as the system accent colour.
  designSystem: ['src/ui/**', 'src/gallery/**/*.svelte'],
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
              NO_GALLERY_IMPORT,
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
              NO_GALLERY_IMPORT,
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
      // Test files share one module registry (`isolate: false` in vite.config.ts), so a module mock would leak into every file after it.
      files: ['src/**/*.test.ts', 'src/lib/testing/**', 'vitest-setup.ts'],
      rules: {
        'no-restricted-properties': [
          'error',
          ...['mock', 'doMock'].map((property) => ({
            object: 'vi',
            property,
            message:
              'Test files share one module registry, so a module mock would leak into later files. Pass a fake in instead (as TauriBackend takes `invoke` and `listen`).',
          })),
        ],
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
      // Nor do components and windows hold the backend through state (`app.backend`), a prop or a helper; wirecatch/no-value-of-type says what it can follow and what is left to review.
      files: ['src/lib/components/**', 'src/lib/window/**/*.svelte', 'src/ui/**'],
      ignores: ['src/**/*.test.ts'],
      rules: {
        'wirecatch/no-value-of-type': [
          'error',
          {
            types: ['Backend', 'DownloadWindowBackend', 'MockBackend', 'TauriBackend'],
            message:
              'Components get what they need from state; add a method to the state class that calls the backend.',
          },
        ],
      },
    },
    {
      // Window components show their window's state; only types of the backend may be imported, for the sample windows that pass it on.
      files: ['src/lib/window/**/*.svelte'],
      rules: {
        'no-restricted-imports': [
          'error',
          {
            patterns: [
              NO_TAURI_IMPORT,
              NO_GALLERY_IMPORT,
              {
                group: ['$lib/backend', '**/backend'],
                allowTypeImports: true,
                message:
                  'Window components get what they need from their state, not from the backend.',
              },
              {
                group: ['$lib/backend/*', '**/backend/*'],
                message: 'Import the backend from `$lib/backend`, not from one implementation.',
              },
            ],
          },
        ],
      },
    },
    {
      // Components show state and report what the person did; the state classes call the backend, and the pages wire the two together.
      files: ['src/lib/components/**', 'src/ui/**'],
      ignores: ['src/**/*.test.ts'],
      rules: {
        'no-restricted-imports': [
          'error',
          {
            patterns: [
              NO_TAURI_IMPORT,
              NO_GALLERY_IMPORT,
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
