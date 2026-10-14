// The preview page's dev server (`pnpm preview`): the extension's pages on sample data in a normal
// browser tab. It is never part of the extension: WXT builds only `src/entrypoints`.
import { svelte } from '@sveltejs/vite-plugin-svelte';
import tailwindcss from '@tailwindcss/vite';
import { fileURLToPath } from 'node:url';
import Icons from 'unplugin-icons/vite';
import { defineConfig } from 'vite';
import { buildDefines } from '../build-values';

export default defineConfig({
  root: fileURLToPath(new URL('.', import.meta.url)),
  plugins: [tailwindcss(), svelte(), Icons({ compiler: 'svelte', autoInstall: false })],
  define: buildDefines(),
  resolve: { alias: { '@': fileURLToPath(new URL('../src', import.meta.url)) } },
  server: { port: 5181, strictPort: true },
});
