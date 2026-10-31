#!/usr/bin/env node
// Makes the Ed25519 key pair that signs the app's update manifests (D111).
//
// Usage: node scripts/release/updater-key.mjs
//
// The public half goes into config/product.toml as `[updates] public_key`; the private half into the repository secret UPDATER_PRIVATE_KEY, and nowhere else. A lost private key means every installed copy must be updated by hand once, to a build that carries a new public key.

import { generateKeyPairSync } from 'node:crypto';

const { publicKey, privateKey } = generateKeyPairSync('ed25519');
// The raw 32 bytes of the public key are the last 32 of its SPKI encoding.
const raw = publicKey.export({ type: 'spki', format: 'der' }).subarray(-32);

console.log(`public_key = "${raw.toString('base64url')}"`);
console.log('');
console.log('UPDATER_PRIVATE_KEY (keep secret):');
console.log(privateKey.export({ type: 'pkcs8', format: 'der' }).toString('base64'));
