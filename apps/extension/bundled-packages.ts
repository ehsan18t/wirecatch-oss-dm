/**
 * The npm packages the extension's bundle holds, read from what the bundler put in it, and the
 * notices file their licences require (D117).
 */
import { execFileSync } from 'node:child_process';
import { realpathSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import type { Plugin } from 'vite';

const EXTENSION = dirname(fileURLToPath(import.meta.url));
const GENERATOR = join(EXTENSION, '..', '..', 'scripts', 'notices', 'npm.mjs');

/** Code the bundler writes into the output with no module of the package's own to show for it. */
const ALWAYS_BUNDLED = ['tailwindcss', 'vite'];

/** Where a bundled file's package is, or nothing for the extension's own code and workspace packages. */
export function packageFolder(moduleId: string): string | undefined {
  const path = moduleId.replaceAll('\\', '/');
  const icon = /~icons\/([^/]+)\//.exec(path);
  if (icon) return `@iconify-json/${icon[1] ?? ''}`;
  return /^.*\/node_modules\/(?:@[^/]+\/)?[^/]+(?=\/)/.exec(path)?.[0];
}

/** Collects across the bundler's runs, one per group of entry points, and writes the file after the last. */
export function bundledPackages() {
  const folders = new Set<string>(ALWAYS_BUNDLED);
  const plugin: Plugin = {
    name: 'wirecatch:bundled-packages',
    apply: 'build',
    generateBundle(_, bundle) {
      for (const output of Object.values(bundle)) {
        if (output.type !== 'chunk') continue;
        for (const id of Object.keys(output.modules)) {
          const folder = packageFolder(id);
          if (folder) folders.add(folder);
        }
      }
    },
  };
  const writeNotices = (file: string, title: string) => {
    const resolved = [...folders].map((folder) =>
      realpathSync(
        folder.includes('/node_modules/') ? folder : join(EXTENSION, 'node_modules', folder),
      ),
    );
    execFileSync(process.execPath, [GENERATOR, file, title, ...new Set(resolved)], {
      stdio: 'inherit',
    });
  };
  return { plugin, writeNotices };
}
