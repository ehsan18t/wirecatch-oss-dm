// Fails a production build when any module from a dev-only folder (the component gallery) is in its module graph. scripts/check-bundle.mjs looks for marker strings in the output, which an import of a gallery component that carries none of them gets past; this reads the bundle's own list of modules instead.
import { isAbsolute, relative } from 'node:path';

/** Whether module `id` is a file inside `folder`; ids may carry a query (`?svelte&type=style`), virtual ids start with `\0`. */
function isInside(id, folder) {
  if (id.startsWith('\0')) return false;
  const path = relative(folder, id.split('?')[0] ?? id);
  return path !== '' && !path.startsWith('..') && !isAbsolute(path);
}

/** The ids of every module the bundle's chunks were built from. */
function bundledModules(bundle) {
  return Object.values(bundle).flatMap((output) =>
    output.type === 'chunk' ? (output.moduleIds ?? Object.keys(output.modules)) : [],
  );
}

/**
 * A Vite plugin that fails the build when a module inside one of `folders` (absolute paths) ends up in the bundle.
 *
 * @param {string[]} folders
 * @returns {import('vite').Plugin}
 */
export function devOnlyModules(folders) {
  return {
    name: 'wirecatch:dev-only-modules',
    apply: 'build',
    generateBundle(_options, bundle) {
      const found = bundledModules(bundle).filter((id) =>
        folders.some((folder) => isInside(id, folder)),
      );
      if (found.length > 0) {
        this.error(
          `Dev-only modules reached the production bundle (import them only from their own folder):\n${[...new Set(found)].join('\n')}`,
        );
      }
    },
  };
}
