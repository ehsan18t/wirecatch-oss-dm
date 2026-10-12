/// <reference types="vitest/config" />
import { sveltekit } from '@sveltejs/kit/vite';
import { svelteTesting } from '@testing-library/svelte/vite';
import { defineConfig } from 'vite';

/** Set by `tauri dev` to serve a device on the network; unset or empty means this machine only. */
const host = process.env.TAURI_DEV_HOST ?? '';

export default defineConfig({
  plugins: [sveltekit(), svelteTesting()],
  // Tauri expects a fixed port and fails if it is taken.
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host === '' ? false : host,
    hmr: host === '' ? undefined : { protocol: 'ws', host, port: 1421 },
    watch: { ignored: ['**/src-tauri/**'] },
  },
  test: {
    environment: 'jsdom',
    include: ['src/**/*.test.ts'],
    setupFiles: ['./vitest-setup.ts'],
    restoreMocks: true,
    // Most of a run went to building a jsdom window for every test file and to compiling the same
    // Svelte modules again on every run. A vm pool builds the window once per worker and still
    // gives each file its own module graph; the module cache keeps compiled modules on disk
    // between runs. Less work per run is what keeps the tests inside their time limit when the
    // machine is busy with other builds.
    pool: 'vmThreads',
    fsModuleCache: true,
    // src/lib/layout.ts reads its lengths from the text of app.css, so tests need the real file.
    css: { include: [/\/src\/app\.css/] },
  },
});
