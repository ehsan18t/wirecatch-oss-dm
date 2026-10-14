// The component gallery is dev-only: the build fails when any of its modules is bundled, found from the bundle's own module list rather than from strings in the output.

import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { devOnlyModules } from '../../scripts/dev-only-modules.mjs';

// Vitest runs from apps/desktop.
const GALLERY = join(process.cwd(), 'src', 'gallery');
const app = (path: string) => join(process.cwd(), path).replaceAll('\\', '/');

function build(moduleIds: string[]): string | null {
  const plugin = devOnlyModules([GALLERY]);
  const hook = plugin.generateBundle;
  if (typeof hook !== 'function') throw new Error('generateBundle is not a function');
  const bundle = { 'main.js': { type: 'chunk', moduleIds } };
  try {
    // The hook is synchronous; its declared type allows a promise.
    void hook.call(
      {
        error: (message: string) => {
          throw new Error(message);
        },
      } as never,
      {} as never,
      bundle as never,
      false,
    );
    return null;
  } catch (error) {
    return error instanceof Error ? error.message : String(error);
  }
}

describe('dev-only modules in a production build', () => {
  it('fail the build when a gallery component is imported, whatever it contains', () => {
    const error = build([
      app('src/pages/main.ts'),
      app('src/gallery/Specimen.svelte'),
      `${app('src/gallery/Specimen.svelte')}?svelte&type=style&lang.css`,
    ]);
    expect(error).toContain('src/gallery/Specimen.svelte');
  });

  it('let the app’s own modules and virtual modules through', () => {
    expect(
      build([
        app('src/pages/main.ts'),
        app('src/lib/gallery-like.ts'),
        app('src/galleryish/x.ts'),
        '\0virtual:icons/lucide/x',
      ]),
    ).toBeNull();
  });

  it('runs on builds only, never on the dev server that serves the gallery', () => {
    expect(devOnlyModules([GALLERY]).apply).toBe('build');
  });
});
