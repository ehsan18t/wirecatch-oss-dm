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
    // src/lib/layout.ts reads its lengths from the text of app.css, so tests need the real file.
    css: { include: [/\/src\/app\.css/] },
  },
});
