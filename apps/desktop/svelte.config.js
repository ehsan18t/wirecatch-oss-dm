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
  },
};

export default config;
