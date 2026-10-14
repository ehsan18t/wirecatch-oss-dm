import { globals, wirecatch } from '@wirecatch/eslint-config';
import svelteConfig from './svelte.config.js';

export default wirecatch({
  tsconfigRootDir: import.meta.dirname,
  globals: { ...globals.browser },
  svelteConfig,
  // The whole package is the design system: nothing in it writes a raw value.
  designSystem: ['src/**'],
  layers: [
    {
      // Every icon in the package goes through Icon, so its size comes from the tokens; Icon itself renders the component it is given. The Tailwind primitives have no classes of their own for a copy to carry, so no button classes are named.
      files: ['src/**/*.svelte'],
      ignores: ['src/primitives/Icon.svelte'],
      rules: { 'wirecatch/use-ui-primitives': ['error', { buttonClasses: {}, icon: 'Icon' }] },
    },
  ],
});
