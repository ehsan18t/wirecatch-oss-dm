#!/usr/bin/env node
// Writes the update manifest of a release (D111): every installer in a folder, with its system, architecture, kind, size and SHA-256, as `update.json`, and its Ed25519 signature as `update.json.sig`, both in that folder.
//
// Usage: UPDATER_PRIVATE_KEY=<base64 PKCS#8> node scripts/release/manifest.mjs <folder>
//
// The installers are named as scripts/package.mjs names them, <product>-<version>-<system>-<architecture>, with the version in config/product.toml. Anything else in the folder is left out of the manifest.

import { createHash, createPrivateKey, sign } from 'node:crypto';
import { readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { ROOT } from '../gate/process.mjs';
import { readProduct } from '../product/read.mjs';

/** The end of each installer's name, after `<product>-<version>-<system>-<architecture>`, and what it is. */
const KINDS = [
  ['-setup.exe', 'setup'],
  ['.AppImage', 'appimage'],
  ['.deb', 'deb'],
  ['.rpm', 'rpm'],
];
const SYSTEMS = ['windows', 'linux'];
const ARCHES = ['x64', 'arm64'];

/** What `name` is, when it is an installer of `product` at `version`; null otherwise. */
export function describe(name, product, version) {
  const prefix = `${product}-${version}-`;
  if (!name.startsWith(prefix)) return null;
  const rest = name.slice(prefix.length);
  for (const [ending, kind] of KINDS) {
    if (!rest.endsWith(ending)) continue;
    const [system, arch, ...more] = rest.slice(0, -ending.length).split('-');
    if (more.length > 0 || !SYSTEMS.includes(system) || !ARCHES.includes(arch)) return null;
    return { system, arch, kind };
  }
  return null;
}

/** The manifest of the installers in `folder`, as the bytes that are signed. */
export function manifest(folder, product, version) {
  const files = readdirSync(folder)
    .sort()
    .flatMap((name) => {
      const what = describe(name, product, version);
      if (!what) return [];
      const bytes = readFileSync(join(folder, name));
      return [
        {
          name,
          ...what,
          size: statSync(join(folder, name)).size,
          sha256: createHash('sha256').update(bytes).digest('hex'),
        },
      ];
    });
  return Buffer.from(`${JSON.stringify({ version, files }, null, 2)}\n`);
}

/** `bytes` signed with the base64 PKCS#8 Ed25519 key `privateKey`, as unpadded base64url. */
export function signature(bytes, privateKey) {
  const key = createPrivateKey({
    key: Buffer.from(privateKey, 'base64'),
    format: 'der',
    type: 'pkcs8',
  });
  return sign(null, bytes, key).toString('base64url');
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const [folder] = process.argv.slice(2);
  const privateKey = process.env.UPDATER_PRIVATE_KEY;
  if (!folder || !privateKey) {
    console.error(
      'Usage: UPDATER_PRIVATE_KEY=<base64 PKCS#8> node scripts/release/manifest.mjs <folder>',
    );
    process.exit(2);
  }
  const product = readProduct(readFileSync(join(ROOT, 'config', 'product.toml'), 'utf8'));
  const bytes = manifest(folder, product.product.name, product.product.version);
  const listed = JSON.parse(bytes.toString()).files;
  if (listed.length === 0) {
    console.error(`No installer of version ${product.product.version} is in ${folder}.`);
    process.exit(1);
  }
  writeFileSync(join(folder, 'update.json'), bytes);
  writeFileSync(join(folder, 'update.json.sig'), signature(bytes, privateKey));
  console.log(`Signed the manifest of ${String(listed.length)} installers in ${folder}.`);
}
