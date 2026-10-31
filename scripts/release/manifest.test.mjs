import assert from 'node:assert/strict';
import { generateKeyPairSync, verify } from 'node:crypto';
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { ROOT } from '../gate/process.mjs';
import { describe, manifest, signature } from './manifest.mjs';

/** The manifest the app's update check reads in its own tests (crates/updates). */
const SAMPLE = readFileSync(join(ROOT, 'crates', 'updates', 'samples', 'update.json'));

test('installers are read from their names, anything else is left out', () => {
  assert.deepEqual(describe('App-1.2.0-beta.1-linux-arm64.AppImage', 'App', '1.2.0-beta.1'), {
    system: 'linux',
    arch: 'arm64',
    kind: 'appimage',
  });
  assert.equal(describe('App-1.1.0-linux-x64.deb', 'App', '1.2.0'), null);
  assert.equal(describe('App-1.2.0-macos-x64.deb', 'App', '1.2.0'), null);
  assert.equal(describe('extension.zip', 'App', '1.2.0'), null);
});

test('the manifest is the one the app reads, signed so the key checks it', () => {
  // Each sample installer holds its kind's name, which gives the sizes and hashes in the sample.
  const folder = mkdtempSync(join(tmpdir(), 'manifest-'));
  for (const { name, kind } of JSON.parse(SAMPLE.toString()).files) {
    writeFileSync(join(folder, name), kind);
  }
  writeFileSync(join(folder, 'notes.txt'), 'not an installer');
  const bytes = manifest(folder, 'App', '1.2.0');
  assert.equal(bytes.toString(), SAMPLE.toString());

  const { publicKey, privateKey } = generateKeyPairSync('ed25519');
  const signed = signature(
    bytes,
    privateKey.export({ type: 'pkcs8', format: 'der' }).toString('base64'),
  );
  assert.ok(verify(null, bytes, publicKey, Buffer.from(signed, 'base64url')));
});
