import type { Plugin } from 'vite';

/** A Vite plugin that fails the build when a module from one of `folders` is bundled; see `dev-only-modules.mjs`. */
export function devOnlyModules(folders: string[]): Plugin;
