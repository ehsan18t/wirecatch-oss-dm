// The preview page's dev server (`pnpm preview`): the extension's pages on sample data in a normal
// browser tab. It is never part of the extension: WXT builds only `src/entrypoints`.
import { fileURLToPath } from 'node:url';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import tailwindcss from '@tailwindcss/vite';
import Icons from 'unplugin-icons/vite';
import { defineConfig } from 'vite';
import { buildDefines } from '../build-values';

export default defineConfig({
  root: fileURLToPath(new URL('.', import.meta.url)),
  plugins: [tailwindcss(), svelte(), Icons({ compiler: 'svelte', autoInstall: false })],
  define: buildDefines(),
  resolve: { alias: { '@': fileURLToPath(new URL('../src', import.meta.url)) } },
  server: { port: 5181, strictPort: true },
  // Every page, so a build serves what the dev server does (the browser tests run on a build).
  build: {
    rollupOptions: {
      input: ['index.html', 'notices.html', 'notices-frame.html'].map((page) =>
        fileURLToPath(new URL(page, import.meta.url)),
      ),
    },
  },
});
