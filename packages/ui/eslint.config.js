import { globals, wirecatch } from '@wirecatch/eslint-config';
import svelteConfig from './svelte.config.js';

export default wirecatch({
  tsconfigRootDir: import.meta.dirname,
  globals: { ...globals.browser },
  svelteConfig,
});
