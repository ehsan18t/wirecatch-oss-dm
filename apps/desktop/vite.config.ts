/// <reference types="vitest/config" />
import { svelte } from '@sveltejs/vite-plugin-svelte';
import tailwindcss from '@tailwindcss/vite';
import { svelteTesting } from '@testing-library/svelte/vite';
import { createHash } from 'node:crypto';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import Icons from 'unplugin-icons/vite';
import { defineConfig, type Plugin } from 'vite';

/** Set by `tauri dev` to serve a device on the network; unset or empty means this machine only. */
const host = process.env.TAURI_DEV_HOST ?? '';

/**
 * What decides how a module compiles besides its own source: the installed versions (the
 * lockfile), the Svelte and TypeScript options and the plugin options in this file. Vitest's
 * module cache key leaves them out and the cache survives `pnpm install`, so they are hashed
 * into the key here.
 */
const COMPILE_INPUTS = [
  '../../pnpm-lock.yaml',
  './svelte.config.js',
  './vite.config.ts',
  './tsconfig.json',
];

/**
 * Every HTML file next to this config is a page of its own: index.html for the main window, and
 * DOWNLOAD_WINDOW_PAGE (in @wirecatch/protocol), which the shell opens for each download window.
 */
const PAGES = readdirSync(import.meta.dirname)
  .filter((file) => file.endsWith('.html'))
  .map((file) => join(import.meta.dirname, file));

function moduleCacheKey(): Plugin {
  const hash = createHash('sha256');
  for (const file of COMPILE_INPUTS) {
    const url = new URL(file, import.meta.url);
    hash.update(file);
    if (existsSync(url)) hash.update(readFileSync(url));
  }
  const key = hash.digest('hex');
  return {
    name: 'wirecatch:module-cache-key',
    configureVitest({ defineCacheKeyGenerator }) {
      defineCacheKeyGenerator(() => key);
    },
  };
}

export default defineConfig({
  plugins: [
    svelte(),
    tailwindcss(),
    Icons({ compiler: 'svelte', autoInstall: false }),
    svelteTesting(),
    moduleCacheKey(),
  ],
  // `$lib` and any other path alias are declared once, in tsconfig.json.
  resolve: { tsconfigPaths: true },
  build: {
    outDir: 'build',
    rolldownOptions: { input: PAGES },
  },
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
    // A test's time limit bounds a hang, not the work: a full-app test that takes a second
    // alone takes many times that on a machine busy with other builds, and Vitest's default
    // of five seconds failed it there. The same two minutes as `PATIENCE` in
    // crates/test-support (docs/ENGINEERING.md, section 7).
    testTimeout: 120_000,
    hookTimeout: 120_000,
    // Compiled modules are kept on disk between runs, so a run does not compile the same Svelte
    // modules again; moduleCacheKey above keys that cache on what changes their output.
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
