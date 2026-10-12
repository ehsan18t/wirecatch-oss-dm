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
    // Compiled modules are kept on disk between runs, so a run does not compile the same Svelte
    // modules again.
    fsModuleCache: true,
    // Building a jsdom window for every test file was the largest share of a run, and when the
    // machine was busy the full-app tests ran past their time limit. Test files share one window
    // per worker instead: Testing Library unmounts after every test, and the suite passes with
    // its files in any order (`--sequence.shuffle.files`). Not a vm pool: there structuredClone
    // returns objects from another realm, which Svelte's $state does not proxy, so the tests
    // would no longer be deeply reactive like the app.
    pool: 'threads',
    isolate: false,
    // What one file stubs must not reach the next; vitest-setup.ts resets the rest of the window,
    // and the lint config bans module mocks, which would leak through the shared module registry.
    unstubGlobals: true,
    unstubEnvs: true,
    // src/lib/layout.ts reads its lengths from the text of app.css, so tests need the real file.
    css: { include: [/\/src\/app\.css/] },
  },
});
