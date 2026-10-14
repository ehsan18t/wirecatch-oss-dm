import { svelte } from '@sveltejs/vite-plugin-svelte';
import { svelteTesting } from '@testing-library/svelte/vite';
import Icons from 'unplugin-icons/vite';
import { defineConfig } from 'vitest/config';

export default defineConfig({
  plugins: [svelte(), Icons({ compiler: 'svelte', autoInstall: false }), svelteTesting()],
  test: {
    environment: 'jsdom',
    include: ['src/**/*.test.ts'],
    setupFiles: ['./src/testing/setup.ts'],
    restoreMocks: true,
    unstubGlobals: true,
  },
});
