// Builds the browser bridge and copies it where the Tauri bundler expects its sidecar, src-tauri/binaries/<name>-<target triple>[.exe]; the bundler installs it next to the app under its plain name, where the bridge and the app look for each other (D19). The name is the one `externalBin` lists in tauri.bundle.conf.json, which is generated from config/product.toml.
//
// Usage: node scripts/prepare-bridge.mjs [--release]. No cargo feature is turned on: both profiles trust the same extensions (D22).

import { execFileSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, readFileSync } from 'node:fs';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { bridgeCargoArgs } from './bridge-args.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const desktop = resolve(here, '..');
const root = resolve(desktop, '..', '..');
const release = process.argv.includes('--release');

// A release build carries the product identity into installers, so its generated files must be current; a placeholder is only warned about, since publishing refuses it (D29, D49).
if (release) {
  try {
    execFileSync(process.execPath, [join(root, 'scripts', 'gate.mjs'), 'release-build'], {
      cwd: root,
      stdio: 'inherit',
    });
  } catch (error) {
    // A check that ran has printed why it failed; one that could not start has not.
    const started =
      typeof error === 'object' && error !== null && 'status' in error && error.status !== null;
    console.error(
      started
        ? 'The product identity is out of date; see the message above.'
        : `Could not run the release check: ${error instanceof Error ? error.message : String(error)}`,
    );
    process.exit(1);
  }
}

/** The bridge's file name, without `.exe`: the one sidecar the bundle configuration lists. */
function bridgeName() {
  const config = JSON.parse(
    readFileSync(join(desktop, 'src-tauri', 'tauri.bundle.conf.json'), 'utf8'),
  );
  const sidecars = config.bundle?.externalBin ?? [];
  if (sidecars.length !== 1) {
    console.error('tauri.bundle.conf.json must list the bridge as its one externalBin.');
    process.exit(1);
  }
  return basename(sidecars[0]);
}
const bridge = bridgeName();

const host = /^host: (.+)$/m.exec(execFileSync('rustc', ['-vV'], { encoding: 'utf8' }))?.[1];
if (!host) {
  console.error('Could not read the target triple from `rustc -vV`. Is Rust installed?');
  process.exit(1);
}

const args = bridgeCargoArgs(release);
if (args.some((a) => /^(?:--features|--all-features|-F)$/.test(a))) {
  console.error(
    'A bundled bridge must never be built with a cargo feature (test-overrides is for tests only).',
  );
  process.exit(1);
}
execFileSync('cargo', args, { cwd: root, stdio: 'inherit' });

const exe = process.platform === 'win32' ? '.exe' : '';
const targetDir = process.env.CARGO_TARGET_DIR
  ? resolve(process.env.CARGO_TARGET_DIR)
  : join(root, 'target');
const built = join(targetDir, release ? 'release' : 'debug', `${bridge}${exe}`);
if (!existsSync(built)) {
  console.error(`The bridge was built, but ${built} is missing.`);
  process.exit(1);
}
const outDir = join(desktop, 'src-tauri', 'binaries');
mkdirSync(outDir, { recursive: true });
const out = join(outDir, `${bridge}-${host}${exe}`);
copyFileSync(built, out);
console.log(`Bridge ready for bundling: ${out}`);
