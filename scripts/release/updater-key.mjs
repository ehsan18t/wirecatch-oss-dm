#!/usr/bin/env node
// Makes the Ed25519 key pair that signs the app's update manifests (D111).
//
// Usage: node scripts/release/updater-key.mjs
//
// In signed mode the public half goes into config/product.toml as `[updates] public_key`; the private half into the secret UPDATER_PRIVATE_KEY of the `updater-signing` environment, and nowhere else. A lost private key means every signed installed copy must be updated by hand once, to a build that carries a new public key.

import { generateKeyPairSync } from 'node:crypto';
import { fileURLToPath } from 'node:url';

/** The raw 32-byte Ed25519 `publicKey` as unpadded base64url, the form `[updates] public_key` takes. */
export function rawPublicKey(publicKey) {
  // The raw key is the last 32 bytes of its SPKI encoding.
  return publicKey.export({ type: 'spki', format: 'der' }).subarray(-32).toString('base64url');
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const { publicKey, privateKey } = generateKeyPairSync('ed25519');
  console.log(`public_key = "${rawPublicKey(publicKey)}"`);
  console.log('');
  console.log('UPDATER_PRIVATE_KEY (keep secret):');
  console.log(privateKey.export({ type: 'pkcs8', format: 'der' }).toString('base64'));
}
