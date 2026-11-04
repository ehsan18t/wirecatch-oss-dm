// The Rust crates an installer carries, read from Cargo.lock through cargo (D117).
import { dirname } from 'node:path';
import { capture, cargoMetadata } from '../gate/process.mjs';

/** What `scripts/package.mjs` builds into an installer. No feature is turned on (D22). */
export const INSTALLER_CRATES = ['wirecatch-desktop', 'wirecatch-bridge'];

/** `name vVERSION`, then what cargo tree appends: a path, `(proc-macro)`, `(*)`. */
const TREE_LINE = /^(\S+) v(\S+)/;

function builtInto(triple) {
  // Build dependencies count: proc macros and build scripts can write their own code into the result.
  const { out } = capture('cargo', [
    'tree',
    '--locked',
    ...INSTALLER_CRATES.flatMap((name) => ['-p', name]),
    '--target',
    triple,
    '--edges',
    'normal,build',
    '--prefix',
    'none',
    '--format',
    '{p}',
  ]);
  return new Set(
    out
      .split('\n')
      .map((line) => TREE_LINE.exec(line))
      .filter(Boolean)
      .map(([, name, version]) => `${name} ${version}`),
  );
}

/** Every crate from a registry in the installer for `triple`. The workspace's own crates are the app, under its own licence. */
export function installerPackages(triple) {
  const built = builtInto(triple);
  return cargoMetadata({ deps: true })
    .packages.filter((pkg) => pkg.source && built.has(`${pkg.name} ${pkg.version}`))
    .map((pkg) => ({
      name: pkg.name,
      version: pkg.version,
      license: pkg.license ?? '',
      repository: pkg.repository,
      authors: pkg.authors,
      dir: dirname(pkg.manifest_path),
    }));
}
