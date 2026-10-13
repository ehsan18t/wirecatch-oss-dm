// The shared ESLint configuration. Each package calls `wirecatch()` with what is specific to it (its globals, ignored folders and layer rules) and gets the same strict baseline as every other package. Change the baseline here, never in a package.
import comments from '@eslint-community/eslint-plugin-eslint-comments';
import js from '@eslint/js';
import prettier from 'eslint-config-prettier';
import svelte from 'eslint-plugin-svelte';
import globals from 'globals';
import ts from 'typescript-eslint';
import { noAssertionOnInput } from './rules/no-assertion-on-input.js';
import { noBrandAssertion } from './rules/no-brand-assertion.js';
import { noMagicNumbers } from './rules/no-magic-numbers.js';
import { noTypeArgumentAssertion } from './rules/no-type-argument-assertion.js';
import { noValueOfType } from './rules/no-value-of-type.js';
import { useUiPrimitives } from './rules/use-ui-primitives.js';

/** Rules of our own, used as `wirecatch/<name>`. */
const plugin = {
  meta: { name: '@wirecatch/eslint-config' },
  rules: {
    'no-assertion-on-input': noAssertionOnInput,
    'no-brand-assertion': noBrandAssertion,
    'no-magic-numbers': noMagicNumbers,
    'no-type-argument-assertion': noTypeArgumentAssertion,
    'no-value-of-type': noValueOfType,
    'use-ui-primitives': useUiPrimitives,
  },
};

/** Numbers that read as themselves and never need a name. */
const OBVIOUS_NUMBERS = [-1, 0, 1, 2];

/** Limits that keep functions small enough to read in one screen. */
const MAX_COMPLEXITY = 12;
const MAX_FUNCTION_LINES = 80;
const MAX_FILE_LINES = 400;
const MAX_DEPTH = 4;
const MAX_PARAMS = 4;

const TEST_FILES = ['**/*.test.ts', '**/*.spec.ts', 'tests/**', 'e2e/**', 'src/**/testing/**'];
const TOOLING_FILES = ['*.config.ts', '*.config.js', 'scripts/**'];
/** What an `untyped` pattern must end with: a JavaScript extension, so it can never match TypeScript or Svelte. */
const UNTYPED_PATTERN = /\.[mc]?js$/;
const UNTYPED_FILES = ['eslint.config.js', 'svelte.config.js', '**/*.mjs'];

/** The rules every package follows, on top of the recommended and strict sets. */
const BASELINE_RULES = {
  // Correctness. TypeScript already reports undefined names, and `no-undef` does not understand type-only globals.
  'no-undef': 'off',
  eqeqeq: ['error', 'always'],
  'no-console': 'error',
  'no-param-reassign': 'error',
  '@typescript-eslint/no-explicit-any': 'error',
  '@typescript-eslint/no-unused-vars': ['error', { argsIgnorePattern: '^_' }],
  '@typescript-eslint/switch-exhaustiveness-check': 'error',
  '@typescript-eslint/consistent-type-imports': 'error',
  '@typescript-eslint/restrict-template-expressions': ['error', { allowNumber: true }],
  // `onclick={() => model.reconnect()}` is clearer than wrapping every handler in braces.
  '@typescript-eslint/no-confusing-void-expression': ['error', { ignoreArrowShorthand: true }],
  // Outside data (`any` or `unknown`) gets its type from a guard that checks it, never from `as` or `!`.
  'wirecatch/no-assertion-on-input': 'error',
  // An assertion that narrows a type claims what nothing checked, and so does a type argument that only says what a call returns (`listen<UiEvent>`); outside data gets its type from a generated guard.
  '@typescript-eslint/no-unsafe-type-assertion': 'error',
  'wirecatch/no-type-argument-assertion': 'error',
  // A branded id comes from the app, a generated guard or its constructor, never from an assertion or a guard written by hand; a protocol type is checked by its generated guard.
  'wirecatch/no-brand-assertion': [
    'error',
    {
      constructors: ['packages/protocol/src/ids.ts'],
      generated: ['packages/protocol/src/generated/'],
    },
  ],
  // Every number that is not obvious gets a name that says what it means; the value of a module constant may be written the way it reads (`8 * 1024`).
  'no-magic-numbers': 'off',
  'wirecatch/no-magic-numbers': [
    'error',
    {
      ignore: OBVIOUS_NUMBERS,
      ignoreArrayIndexes: true,
      ignoreDefaultValues: true,
      ignoreEnums: true,
      ignoreNumericLiteralTypes: true,
      ignoreReadonlyClassProperties: true,
      ignoreTypeIndexes: true,
    },
  ],
  // A disable comment names its rules, says why after `--`, and switches them back on: an exception is one reasoned decision about a few lines, never a file or every rule.
  '@eslint-community/eslint-comments/require-description': ['error', { ignore: ['eslint-enable'] }],
  '@eslint-community/eslint-comments/no-unlimited-disable': 'error',
  '@eslint-community/eslint-comments/disable-enable-pair': 'error',
  // The configuration lives in eslint.config.js, never in a comment: no rule settings, globals or environments written as comments, which would switch a rule off without the reason a disable comment needs.
  '@eslint-community/eslint-comments/no-use': [
    'error',
    { allow: ['eslint-disable', 'eslint-disable-next-line', 'eslint-enable'] },
  ],
  // Size limits.
  complexity: ['error', MAX_COMPLEXITY],
  'max-depth': ['error', MAX_DEPTH],
  'max-params': ['error', MAX_PARAMS],
  'max-lines-per-function': [
    'error',
    { max: MAX_FUNCTION_LINES, skipBlankLines: true, skipComments: true },
  ],
  'max-lines': ['error', { max: MAX_FILE_LINES, skipBlankLines: true, skipComments: true }],
  // Svelte.
  'svelte/require-each-key': 'error',
  'svelte/no-at-html-tags': 'error',
  'svelte/prefer-svelte-reactivity': 'error',
  'svelte/no-unused-svelte-ignore': 'error',
  'svelte/button-has-type': 'error',
};

/**
 * Builds the configuration for one package.
 *
 * @param {object} options
 * @param {string} options.tsconfigRootDir The package folder, usually `import.meta.dirname`.
 * @param {Record<string, boolean | 'readonly' | 'writable'>} options.globals Runtime globals.
 * @param {string[]} [options.ignores] Folders the linter skips, such as build output.
 * @param {object} [options.svelteConfig] The package's `svelte.config.js`, when it has one.
 * @param {import('eslint').Linter.Config[]} [options.layers] Import rules that keep each layer of the package from reaching into the wrong one.
 * @param {string[]} [options.untyped] More JavaScript files linted without type information, like the scripts and configuration files every package has. Each pattern must end in `.js`, `.mjs` or `.cjs`: TypeScript and Svelte are always type-checked.
 * @returns {import('eslint').Linter.Config[]}
 */
export function wirecatch({
  tsconfigRootDir,
  globals: runtimeGlobals,
  ignores = [],
  svelteConfig,
  layers = [],
  untyped = [],
}) {
  const typed = untyped.filter((pattern) => !UNTYPED_PATTERN.test(pattern));
  if (typed.length > 0) {
    throw new Error(
      `wirecatch({ untyped }) takes JavaScript patterns only (ending in .js, .mjs or .cjs); TypeScript and Svelte are always type-checked: ${typed.join(', ')}`,
    );
  }
  const untypedFiles = [...UNTYPED_FILES, ...untyped];
  return ts.config(
    { ignores: ['node_modules/', ...ignores] },
    js.configs.recommended,
    ...ts.configs.strictTypeChecked,
    ...ts.configs.stylisticTypeChecked,
    ...svelte.configs.recommended,
    prettier,
    ...svelte.configs.prettier,
    {
      languageOptions: {
        globals: runtimeGlobals,
        parserOptions: { projectService: true, tsconfigRootDir, extraFileExtensions: ['.svelte'] },
      },
      plugins: { wirecatch: plugin, '@eslint-community/eslint-comments': comments },
      linterOptions: { reportUnusedDisableDirectives: 'error' },
      rules: BASELINE_RULES,
    },
    {
      files: ['**/*.svelte', '**/*.svelte.ts'],
      languageOptions: { parserOptions: { parser: ts.parser, svelteConfig } },
    },
    {
      // Tests name their fixtures inline and may be as long as the scenario needs.
      files: TEST_FILES,
      rules: {
        'wirecatch/no-magic-numbers': 'off',
        'wirecatch/no-assertion-on-input': 'off',
        '@typescript-eslint/no-unsafe-type-assertion': 'off',
        'wirecatch/no-type-argument-assertion': 'off',
        '@typescript-eslint/no-non-null-assertion': 'off',
        'max-lines-per-function': 'off',
        'max-lines': 'off',
      },
    },
    {
      files: TOOLING_FILES,
      languageOptions: { globals: globals.node },
      rules: { 'no-console': 'off', 'wirecatch/no-magic-numbers': 'off' },
    },
    { files: untypedFiles, ...ts.configs.disableTypeChecked },
    {
      files: untypedFiles,
      rules: {
        'wirecatch/no-assertion-on-input': 'off',
        'wirecatch/no-brand-assertion': 'off',
        'wirecatch/no-type-argument-assertion': 'off',
      },
    },
    ...layers,
  );
}

export { globals };
