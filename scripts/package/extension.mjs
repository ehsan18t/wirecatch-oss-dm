// The browser extension for Chromium and Firefox: unpacked in `EXTENSION_OUT`, where a browser loads it, and zipped into the packages folder.
import { copyFileSync, mkdirSync, readdirSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { pnpm, ROOT } from '../gate/process.mjs';

/** Where WXT writes the extension: `outDir` in extension/wxt.config.ts. */
export const EXTENSION_OUT = join(ROOT, 'target', 'extension');

/** The unpacked builds a browser loads, by folder name. */
export const EXTENSION_BUILDS = ['chrome-mv3', 'firefox-mv3'];

const zips = () => readdirSync(EXTENSION_OUT).filter((name) => name.endsWith('.zip'));

/** Builds and zips the extension for both browsers, and copies the zips into `packages`. */
export function buildExtension(packages) {
  mkdirSync(EXTENSION_OUT, { recursive: true });
  // Zips of an earlier version would otherwise be copied along.
  for (const zip of zips()) rmSync(join(EXTENSION_OUT, zip));
  pnpm('--filter', '@wirecatch/extension', 'exec', 'wxt', 'zip');
  pnpm('--filter', '@wirecatch/extension', 'exec', 'wxt', 'zip', '-b', 'firefox', '--mv3');
  mkdirSync(packages, { recursive: true });
  for (const zip of zips()) copyFileSync(join(EXTENSION_OUT, zip), join(packages, zip));
}
