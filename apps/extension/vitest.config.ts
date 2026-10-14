import { svelte } from '@sveltejs/vite-plugin-svelte';
import Icons from 'unplugin-icons/vite';
import { defineConfig } from 'vitest/config';
import { WxtVitest } from 'wxt/testing/vitest-plugin';
import { buildDefines } from './build-values';

export default defineConfig({
  // WxtVitest does not run the Svelte module's Vite setup or wxt.config.ts's plugins, so the Svelte
  // plugin and the icons the design system's components import are added here.
  plugins: [WxtVitest(), svelte(), Icons({ compiler: 'svelte', autoInstall: false })],
  // WxtVitest does not apply wxt.config.ts's `vite.define`, so the same values are given here.
  define: buildDefines(),
  resolve: {
    // Load Svelte's browser build so components can be mounted in jsdom.
    conditions: ['browser'],
  },
  test: {
    environment: 'jsdom',
    include: ['tests/**/*.test.ts'],
    setupFiles: ['./tests/setup.ts'],
    restoreMocks: true,
  },
});
