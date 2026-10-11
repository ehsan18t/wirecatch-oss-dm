// Builds the browser bridge and puts it where the Tauri bundler expects a sidecar:
// src-tauri/binaries/wirecatch-bridge-<target triple>[.exe]. The bundler installs it next to
// wirecatch-app as plain `wirecatch-bridge[.exe]`, which is where the bridge and the app look
// for each other (D19).
//
// Usage: node scripts/prepare-bridge.mjs [--release]. Without --release the bridge also trusts
// the development extension (cargo feature dev-extension); with it, only the store extensions.

import { execFileSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { bridgeCargoArgs } from './bridge-args.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const desktop = resolve(here, '..');
const root = resolve(desktop, '..', '..');
const release = process.argv.includes('--release');

const host = /^host: (.+)$/m.exec(execFileSync('rustc', ['-vV'], { encoding: 'utf8' }))?.[1];
if (!host) {
  console.error('Could not read the target triple from `rustc -vV`. Is Rust installed?');
  process.exit(1);
}

// Debug builds also trust the development extension; release builds only the store ones.
const args = bridgeCargoArgs(release);
if (release && args.some((a) => a.includes('dev-extension'))) {
  console.error('A release bridge must never be built with the dev-extension feature.');
  process.exit(1);
}
execFileSync('cargo', args, { cwd: root, stdio: 'inherit' });

const exe = process.platform === 'win32' ? '.exe' : '';
const targetDir = process.env.CARGO_TARGET_DIR
  ? resolve(process.env.CARGO_TARGET_DIR)
  : join(root, 'target');
const built = join(targetDir, release ? 'release' : 'debug', `wirecatch-bridge${exe}`);
if (!existsSync(built)) {
  console.error(`The bridge was built, but ${built} is missing.`);
  process.exit(1);
}
const outDir = join(desktop, 'src-tauri', 'binaries');
mkdirSync(outDir, { recursive: true });
const out = join(outDir, `wirecatch-bridge-${host}${exe}`);
copyFileSync(built, out);
console.log(`Bridge ready for bundling: ${out}`);
