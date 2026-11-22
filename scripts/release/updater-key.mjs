#!/usr/bin/env node
// Makes the Ed25519 key pair that signs the app's update manifests (D111).
//
// Usage: node scripts/release/updater-key.mjs
//
// In signed mode the public half goes into config/product.toml as `[updates] public_key`. Store the private half as UPDATER_PRIVATE_KEY in the updater-signing environment and keep a secure backup. If the key is lost, users of signed builds need a manual install of a build with a new public key.

import { generateKeyPairSync } from 'node:crypto';

const { publicKey, privateKey } = generateKeyPairSync('ed25519');
// The raw 32 bytes of the public key are the last 32 of its SPKI encoding.
const raw = publicKey.export({ type: 'spki', format: 'der' }).subarray(-32);

console.log(`public_key = "${raw.toString('base64url')}"`);
console.log('');
console.log('UPDATER_PRIVATE_KEY (keep secret):');
console.log(privateKey.export({ type: 'pkcs8', format: 'der' }).toString('base64'));
