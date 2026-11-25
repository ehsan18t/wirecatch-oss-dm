#!/usr/bin/env node
// Builds what Wirecatch ships, from the repository root: `node scripts/build.mjs [app|extension] [--target <triple>]`, or the same after `pnpm build`.
//
// With no part named it builds both. The extension builds unpacked into target/extension; the app's installers and the extension's zips land in target/packages. `--target` picks the app's Rust target (crates/desktop/PACKAGING.md).
import { execFileSync } from 'node:child_process';
import { readdirSync } from 'node:fs';
import { join, relative } from 'node:path';
import { GateError, ROOT } from './gate/process.mjs';
import { buildApp, hostTriple, OUT } from './package/app.mjs';
import { buildExtension, EXTENSION_BUILDS, EXTENSION_OUT } from './package/extension.mjs';
import { isInstaller } from './release/installers.mjs';

const USAGE = 'Usage: node scripts/build.mjs [app|extension] [--target <rust target triple>]';

function fail(message) {
  console.error(message);
  process.exit(1);
}

function parse(args) {
  const parts = new Set();
  let triple = null;
  for (let i = 0; i < args.length; i++) {
    const arg = args[i];
    if (arg === '--target') {
      triple = args[++i] ?? fail(`--target needs a target triple.\n${USAGE}`);
    } else if (arg === 'app' || arg === 'extension') {
      parts.add(arg);
    } else {
      fail(`Unknown argument ${arg}.\n${USAGE}`);
    }
  }
  if (parts.size === 0) {
    parts.add('extension');
    parts.add('app');
  }
  if (triple !== null && !parts.has('app')) fail(`--target applies to the app only.\n${USAGE}`);
  return { parts, triple };
}

const shown = (path) => relative(ROOT, path).replaceAll('\\', '/');

const { parts, triple } = parse(process.argv.slice(2));
try {
  // Fails when a file generated from config/product.toml is out of date, and warns while it holds a placeholder (D29, D49).
  execFileSync(process.execPath, [join(ROOT, 'scripts', 'gate.mjs'), 'release-build'], {
    cwd: ROOT,
    stdio: 'inherit',
  });
} catch {
  fail('The product identity is out of date; see the message above.');
}
try {
  if (parts.has('extension')) buildExtension(OUT);
  if (parts.has('app')) await buildApp(triple ?? hostTriple());
} catch (error) {
  if (error instanceof GateError) fail(error.message);
  throw error;
}

if (parts.has('extension')) {
  console.log(
    `\nUnpacked extension: ${EXTENSION_BUILDS.map((build) => shown(join(EXTENSION_OUT, build))).join(', ')}`,
  );
}
const built = readdirSync(OUT).filter((name) => isInstaller(name) || name.endsWith('.zip'));
console.log(`Packages in ${shown(OUT)}:\n${built.map((name) => `  ${name}`).join('\n')}`);
