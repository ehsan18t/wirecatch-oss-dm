import adapter from '@sveltejs/adapter-static';
import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';

/** @type {import('@sveltejs/kit').Config} */
const config = {
  preprocess: vitePreprocess(),
  compilerOptions: {
    runes: true,
  },
  kit: {
    // Tauri serves the built files and there is no server, so every route falls back to the one page.
    adapter: adapter({ fallback: 'index.html' }),
    typescript: {
      // The generated tsconfig only covers `src`; the browser tests and tool configs are type-checked too. Paths are relative to `.svelte-kit`.
      config(tsconfig) {
        tsconfig.include.push(
          '../e2e/**/*.ts',
          '../playwright.config.ts',
          '../playwright.perf.config.ts',
          '../vitest-setup.ts',
        );
      },
    },
  },
};

export default config;
