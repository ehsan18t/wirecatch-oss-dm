#!/usr/bin/env node
// Validates release artifacts and writes their manifest for the next workflow jobs (D111).

import { createHash } from 'node:crypto';
import { readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { githubRepository, readProduct } from '../product/read.mjs';
import { installerNames, isInstaller } from './installers.mjs';

/** What `name` is, when it is an installer of `product` at `version`; null otherwise. */
export function describe(name, product, version) {
  const installer = installerNames(product, version).find((candidate) => candidate.name === name);
  if (!installer) return null;
  const { system, arch, kind } = installer;
  return { system, arch, kind };
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
          size: bytes.length,
          sha256: createHash('sha256').update(bytes).digest('hex'),
        },
      ];
    });
  return Buffer.from(`${JSON.stringify({ version, files }, null, 2)}\n`);
}

export function prepareRelease(folder, { product, links }, repository, tag) {
  if (githubRepository(links.repository)?.toLowerCase() !== repository?.toLowerCase()) {
    throw new Error('The workflow repository must match links.repository in config/product.toml.');
  }
  if (tag !== `v${product.version}`) {
    throw new Error(
      `The tag ${tag} does not match version ${product.version} in config/product.toml.`,
    );
  }
  const names = readdirSync(folder).filter(isInstaller).sort();
  const expected = installerNames(product.name, product.version)
    .map(({ name }) => name)
    .sort();
  if (names.length !== expected.length || names.some((name, index) => name !== expected[index])) {
    const missing = expected.filter((name) => !names.includes(name));
    const unexpected = names.filter((name) => !expected.includes(name));
    throw new Error(
      `Wrong installer set. Missing: ${missing.join(', ') || 'none'}. Unexpected: ${unexpected.join(', ') || 'none'}.`,
    );
  }
  const bytes = manifest(folder, product.name, product.version);
  const empty = JSON.parse(bytes.toString()).files.find((file) => file.size === 0);
  if (empty) throw new Error(`The installer ${empty.name} is empty.`);
  return bytes;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  try {
    const [folder, output] = process.argv.slice(2);
    if (!folder || !output)
      throw new Error('Usage: node scripts/release/manifest.mjs <folder> <output>');
    const product = readProduct(readFileSync('config/product.toml', 'utf8'));
    const bytes = prepareRelease(
      folder,
      product,
      process.env.GITHUB_REPOSITORY,
      process.env.GITHUB_REF_NAME,
    );
    writeFileSync(output, bytes);
    console.log(`verification=${product.updates.verification}`);
    console.log(`public_key=${product.updates.public_key ?? ''}`);
    console.log(`prerelease=${String(product.product.version.includes('-'))}`);
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 1;
  }
}
