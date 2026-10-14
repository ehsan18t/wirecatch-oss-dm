import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';

/** Shared by Vitest, svelte-check and ESLint; each app compiles these components with its own. */
export default {
  preprocess: vitePreprocess(),
  compilerOptions: {
    runes: true,
  },
};
