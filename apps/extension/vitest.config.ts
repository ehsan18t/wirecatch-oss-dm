import { svelte } from '@sveltejs/vite-plugin-svelte';
import { defineConfig } from 'vitest/config';
import { WxtVitest } from 'wxt/testing/vitest-plugin';

export default defineConfig({
  // WxtVitest does not run the Svelte module's Vite setup, so the Svelte plugin is added here.
  plugins: [WxtVitest(), svelte()],
  resolve: {
    // Load Svelte's browser build so components can be mounted in jsdom.
    conditions: ['browser'],
  },
  test: {
    environment: 'jsdom',
    include: ['tests/**/*.test.ts'],
    restoreMocks: true,
    // src/lib/theme.ts reads colors from the text of the popup's stylesheet, so tests need the
    // real file.
    css: { include: [/\/popup\/style\.css/] },
  },
});
