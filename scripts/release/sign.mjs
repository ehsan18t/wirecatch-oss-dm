// The protected signing job uses only Node's standard library.
import { createPrivateKey, createPublicKey, sign, verify } from 'node:crypto';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { MANIFEST_FILE, SIGNATURE_FILE } from './update-files.mjs';

export function signature(bytes, privateKey, expectedPublicKey) {
  const key = createPrivateKey({
    key: Buffer.from(privateKey, 'base64'),
    format: 'der',
    type: 'pkcs8',
  });
  if (key.asymmetricKeyType !== 'ed25519') throw new Error('The updater key must be Ed25519.');
  const publicKey = createPublicKey(key);
  const encoded = publicKey
    .export({ type: 'spki', format: 'der' })
    .subarray(-32)
    .toString('base64url');
  const signed = sign(null, bytes, key);
  if (encoded !== expectedPublicKey || !verify(null, bytes, publicKey, signed)) {
    throw new Error(
      'UPDATER_PRIVATE_KEY does not sign for updates.public_key in config/product.toml.',
    );
  }
  return signed.toString('base64url');
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const [input, output] = process.argv.slice(2);
  const { UPDATER_PRIVATE_KEY: privateKey, UPDATER_PUBLIC_KEY: publicKey } = process.env;
  if (!input || !output || !privateKey || !publicKey) {
    console.error('Signing needs a prepared manifest, an output folder and both updater keys.');
    process.exit(1);
  }
  const bytes = readFileSync(input);
  let signed;
  try {
    signed = signature(bytes, privateKey, publicKey);
  } catch {
    console.error('UPDATER_PRIVATE_KEY must be the Ed25519 private key for updates.public_key.');
    process.exit(1);
  }
  mkdirSync(output, { recursive: true });
  writeFileSync(join(output, MANIFEST_FILE), bytes);
  writeFileSync(join(output, SIGNATURE_FILE), signed);
}
