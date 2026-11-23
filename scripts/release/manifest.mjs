#!/usr/bin/env node
// Checks a release's installers and, in signed mode, writes their manifest `update.json` into the folder for the signing job (D111, D288). Prints the update mode and public key as `name=value` lines for `$GITHUB_OUTPUT`.
//
// Usage: GITHUB_REPOSITORY=<owner/name> GITHUB_REF_NAME=v<version> node scripts/release/manifest.mjs <folder>

import { createHash } from 'node:crypto';
import { readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { ROOT } from '../gate/process.mjs';
import { githubRepository, readProduct } from '../product/read.mjs';
import { installerNames, isInstaller } from './installers.mjs';
import { MANIFEST_FILE } from './update-files.mjs';

/** What `name` is, when it is an installer of `product` at `version`; null otherwise. */
export function describe(name, product) {
  const installer = installerNames(product).find((candidate) => candidate.name === name);
  if (!installer) return null;
  const { system, arch, kind } = installer;
  return { system, arch, kind };
}

/** The manifest of the installers in `folder`, as the bytes that are signed. */
export function manifest(folder, product) {
  const files = readdirSync(folder)
    .sort()
    .flatMap((name) => {
      const what = describe(name, product);
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
  return Buffer.from(`${JSON.stringify({ version: product.version, files }, null, 2)}\n`);
}

/** Checks that `folder` holds exactly the installers of this tag's version, built in this repository, and writes their manifest in signed mode. */
export function prepareRelease(folder, { product, links, updates }, { repository, tag }) {
  if (githubRepository(links.repository)?.toLowerCase() !== repository?.toLowerCase()) {
    throw new Error('The workflow repository must match links.repository in config/product.toml.');
  }
  if (tag !== `v${product.version}`) {
    throw new Error(
      `The tag ${tag} does not match version ${product.version} in config/product.toml.`,
    );
  }
  const names = readdirSync(folder).filter(isInstaller).sort();
  const expected = installerNames(product)
    .map(({ name }) => name)
    .sort();
  if (names.length !== expected.length || names.some((name, index) => name !== expected[index])) {
    const missing = expected.filter((name) => !names.includes(name));
    const unexpected = names.filter((name) => !expected.includes(name));
    throw new Error(
      `Wrong installer set. Missing: ${missing.join(', ') || 'none'}. Unexpected: ${unexpected.join(', ') || 'none'}.`,
    );
  }
  const empty = names.find((name) => statSync(join(folder, name)).size === 0);
  if (empty) throw new Error(`The installer ${empty} is empty.`);
  if (updates.verification === 'signed') {
    writeFileSync(join(folder, MANIFEST_FILE), manifest(folder, product));
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  try {
    const [folder] = process.argv.slice(2);
    if (!folder) throw new Error('Usage: node scripts/release/manifest.mjs <folder>');
    const product = readProduct(readFileSync(join(ROOT, 'config', 'product.toml'), 'utf8'));
    prepareRelease(folder, product, {
      repository: process.env.GITHUB_REPOSITORY,
      tag: process.env.GITHUB_REF_NAME,
    });
    console.log(`verification=${product.updates.verification}`);
    console.log(`public_key=${product.updates.public_key ?? ''}`);
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 1;
  }
}
