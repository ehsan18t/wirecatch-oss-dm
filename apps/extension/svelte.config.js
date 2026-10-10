import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';

/** Shared by the WXT build, svelte-check and ESLint. */
export default {
  preprocess: vitePreprocess(),
  compilerOptions: {
    runes: true,
  },
};
