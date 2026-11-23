import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { generateKeyPairSync, verify } from 'node:crypto';
import { existsSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { ROOT } from '../gate/process.mjs';
import { installerName, installerNames } from './installers.mjs';
import { describe, prepareRelease } from './manifest.mjs';
import { signature } from './sign.mjs';
import { MANIFEST_FILE, SIGNATURE_FILE } from './update-files.mjs';
import { rawPublicKey } from './updater-key.mjs';

/** The manifest the app's update check reads in its own tests (crates/updates). */
const SAMPLE = readFileSync(join(ROOT, 'crates', 'updates', 'samples', 'update.json'));
const { version: SAMPLE_VERSION, files: SAMPLE_FILES } = JSON.parse(SAMPLE.toString());
const links = { repository: 'https://github.com/owner/app' };
const release = { repository: 'owner/app', tag: `v${SAMPLE_VERSION}` };

test('installers are read from their names, anything else is left out', () => {
  const product = { slug: 'app', version: '1.2.0-beta.1' };
  assert.deepEqual(describe('App-1.2.0-beta.1-linux-arm64.AppImage', product), {
    system: 'linux',
    arch: 'arm64',
    kind: 'appimage',
  });
  assert.equal(describe('App-1.1.0-linux-x64.deb', product), null);
  assert.equal(describe('App-1.2.0-beta.1-linux-x64-setup.exe', product), null);
});

test('a signed release carries the manifest the app reads, signed with the key', () => {
  const folder = mkdtempSync(join(tmpdir(), 'release-'));
  const product = { slug: 'app', name: 'App / Desktop Preview', version: SAMPLE_VERSION };
  // Each installer holds its kind's name, which gives the sizes and hashes in the sample.
  for (const { name, arch, kind } of SAMPLE_FILES) {
    assert.equal(installerName(product, arch, kind), name);
    writeFileSync(join(folder, name), kind);
  }
  writeFileSync(join(folder, 'extension.zip'), 'not an installer');
  prepareRelease(folder, { product, links, updates: { verification: 'signed' } }, release);
  const bytes = readFileSync(join(folder, MANIFEST_FILE));
  assert.deepEqual(bytes, SAMPLE);

  const { publicKey, privateKey } = generateKeyPairSync('ed25519');
  const secret = privateKey.export({ type: 'pkcs8', format: 'der' }).toString('base64');
  const other = rawPublicKey(generateKeyPairSync('ed25519').publicKey);
  assert.throws(() => signature(bytes, secret, other), /not the Ed25519 key/);
  const run = spawnSync(process.execPath, [join(ROOT, 'scripts', 'release', 'sign.mjs'), folder], {
    env: { UPDATER_PRIVATE_KEY: secret, UPDATER_PUBLIC_KEY: rawPublicKey(publicKey) },
    encoding: 'utf8',
  });
  assert.equal(run.status, 0, run.stderr);
  const signed = Buffer.from(readFileSync(join(folder, SIGNATURE_FILE), 'utf8'), 'base64url');
  assert.ok(verify(null, bytes, publicKey, signed));
});

test('a GitHub release needs the exact installer set, tag and repository, and no manifest', () => {
  const folder = mkdtempSync(join(tmpdir(), 'release-'));
  const product = { slug: 'app', version: '1.2.0-beta.1' };
  const github = { product, links, updates: { verification: 'github' } };
  const tag = 'v1.2.0-beta.1';
  const prepare = () => prepareRelease(folder, github, { repository: 'owner/app', tag });
  assert.throws(prepare, /Missing:/);
  for (const { name } of installerNames(product)) {
    writeFileSync(join(folder, name), 'installer');
  }
  writeFileSync(join(folder, 'extension.zip'), 'extension');
  prepare();
  assert.equal(existsSync(join(folder, MANIFEST_FILE)), false);
  assert.throws(
    () => prepareRelease(folder, github, { repository: 'owner/other', tag }),
    /repository must match/,
  );
  assert.throws(
    () => prepareRelease(folder, github, { repository: 'owner/app', tag: 'v1.2.0' }),
    /tag.*does not match/,
  );
  const [first] = installerNames(product);
  writeFileSync(join(folder, first.name), '');
  assert.throws(prepare, /is empty/);
  writeFileSync(join(folder, first.name), 'installer');
  writeFileSync(join(folder, 'App-1.2.0-beta.1-windows-arm64.rpm'), 'wrong format');
  assert.throws(prepare, /Unexpected: App-1.2.0-beta.1-windows-arm64.rpm/);
  writeFileSync(join(folder, 'Extra.EXE'), 'unexpected installer');
  assert.throws(prepare, /Extra\.EXE/);
});
