#!/usr/bin/env node
// Signs a release's `update.json` in the protected signing job, writing `update.json.sig` beside it (D111, D288). Node's standard library only: the job installs no packages while it holds the key.
//
// Usage: UPDATER_PRIVATE_KEY=<base64 PKCS#8> UPDATER_PUBLIC_KEY=<base64url> node scripts/release/sign.mjs <folder>

import { createPrivateKey, createPublicKey, sign } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { MANIFEST_FILE, SIGNATURE_FILE } from './update-files.mjs';
import { rawPublicKey } from './updater-key.mjs';

/** `bytes` signed with the base64 PKCS#8 Ed25519 key `privateKey`, as unpadded base64url, if that key is the one for `publicKey`. */
export function signature(bytes, privateKey, publicKey) {
  const key = createPrivateKey({
    key: Buffer.from(privateKey, 'base64'),
    format: 'der',
    type: 'pkcs8',
  });
  if (key.asymmetricKeyType !== 'ed25519' || rawPublicKey(createPublicKey(key)) !== publicKey) {
    throw new Error(
      'UPDATER_PRIVATE_KEY is not the Ed25519 key for updates.public_key in config/product.toml.',
    );
  }
  return sign(null, bytes, key).toString('base64url');
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const [folder] = process.argv.slice(2);
  const { UPDATER_PRIVATE_KEY: privateKey, UPDATER_PUBLIC_KEY: publicKey } = process.env;
  try {
    if (!folder || !privateKey || !publicKey) {
      throw new Error(
        'Signing needs the release folder, UPDATER_PRIVATE_KEY and UPDATER_PUBLIC_KEY.',
      );
    }
    const bytes = readFileSync(join(folder, MANIFEST_FILE));
    writeFileSync(join(folder, SIGNATURE_FILE), signature(bytes, privateKey, publicKey));
  } catch (error) {
    // Node's key errors never quote the key.
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 1;
  }
}
