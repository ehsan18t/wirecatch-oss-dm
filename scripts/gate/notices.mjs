// The third-party notices of every installer can be generated from the lockfile, and the extension's builds carry theirs (D117).
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { installerPackages } from '../notices/cargo.mjs';
import { shippedTargets } from '../notices/licences.mjs';
import { renderNotices } from '../notices/render.mjs';
import { GateError, ROOT } from './process.mjs';

/** What `pnpm build` writes in the extension's output folder. */
const EXTENSION_BUILDS = ['chrome-mv3', 'firefox-mv3'];

function notices() {
  for (const triple of shippedTargets()) {
    const packages = installerPackages(triple);
    const { length } = renderNotices({ title: triple, packages });
    console.log(`${triple}: ${String(packages.length)} crates, ${String(length)} characters.`);
  }
}

/** The extension writes its file from a build hook, which a WXT update could stop calling without any error. */
export function extensionNoticesBuilt() {
  const product = readFileSync(join(ROOT, 'config', 'product.toml'), 'utf8');
  const file = /^notices = "(.+)"/m.exec(product)?.[1];
  const missing = EXTENSION_BUILDS.filter(
    (build) => !file || !existsSync(join(ROOT, 'apps', 'extension', '.output', build, file)),
  );
  if (missing.length > 0) {
    throw new GateError(
      `The extension's ${missing.join(' and ')} build has no ${String(file)}: the build hook in apps/extension/wxt.config.ts did not write it.`,
    );
  }
}

export const NOTICES_CHECKS = { notices };
