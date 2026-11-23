import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { generateKeyPairSync, verify } from 'node:crypto';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { ROOT } from '../gate/process.mjs';
import { withUpdateConfig } from '../product/identity-samples.mjs';
import { githubRepository, readProduct } from '../product/read.mjs';
import { prepareDraft } from './draft.mjs';
import { installerName, installerNames } from './installers.mjs';
import { describe, manifest, prepareRelease } from './manifest.mjs';
import { signature } from './sign.mjs';
import { MANIFEST_FILE, SIGNATURE_FILE } from './update-files.mjs';

/** The manifest the app's update check reads in its own tests (crates/updates). */
const SAMPLE = readFileSync(join(ROOT, 'crates', 'updates', 'samples', 'update.json'));

test('the product display name does not change the published installer names', () => {
  const folder = mkdtempSync(join(tmpdir(), 'release-name-'));
  const { version, files } = JSON.parse(SAMPLE.toString());
  const product = { slug: 'app', name: 'App / Desktop Preview', version };
  const links = { repository: 'https://github.com/owner/app' };
  for (const { name, arch, kind } of files) {
    assert.equal(installerName(product, arch, kind), name);
    writeFileSync(join(folder, name), kind);
  }
  assert.deepEqual(prepareRelease(folder, { product, links }, 'owner/app', `v${version}`), SAMPLE);
});

test('installers are read from their names, anything else is left out', () => {
  const product = { slug: 'app', version: '1.2.0' };
  const beta = { ...product, version: '1.2.0-beta.1' };
  assert.deepEqual(describe('App-1.2.0-beta.1-linux-arm64.AppImage', beta), {
    system: 'linux',
    arch: 'arm64',
    kind: 'appimage',
  });
  assert.equal(describe('App-1.1.0-linux-x64.deb', product), null);
  assert.equal(describe('App-1.2.0-macos-x64.deb', product), null);
  assert.equal(describe('App-1.2.0-windows-arm64.deb', product), null);
  assert.equal(describe('App-1.2.0-linux-x64-setup.exe', product), null);
  assert.equal(describe('extension.zip', product), null);
});

test('the manifest is the one the app reads, signed so the key checks it', () => {
  // Each sample installer holds its kind's name, which gives the sizes and hashes in the sample.
  const folder = mkdtempSync(join(tmpdir(), 'manifest-'));
  const { version, files } = JSON.parse(SAMPLE.toString());
  for (const { name, kind } of files) {
    writeFileSync(join(folder, name), kind);
  }
  writeFileSync(join(folder, 'notes.txt'), 'not an installer');
  const bytes = manifest(folder, { slug: 'app', version });
  assert.equal(bytes.toString(), SAMPLE.toString());

  const { publicKey, privateKey } = generateKeyPairSync('ed25519');
  const key = publicKey.export({ type: 'spki', format: 'der' }).subarray(-32).toString('base64url');
  const secret = privateKey.export({ type: 'pkcs8', format: 'der' }).toString('base64');
  const signed = signature(bytes, secret, key);
  assert.ok(verify(null, bytes, publicKey, Buffer.from(signed, 'base64url')));

  const other = generateKeyPairSync('ed25519').publicKey;
  const otherKey = other
    .export({ type: 'spki', format: 'der' })
    .subarray(-32)
    .toString('base64url');
  assert.throws(() => signature(bytes, secret, otherKey), /does not sign for updates.public_key/);
});

test('release preparation requires the exact installer set, tag and repository without a key', () => {
  const folder = mkdtempSync(join(tmpdir(), 'release-'));
  const product = { slug: 'app', version: '1.2.0-beta.1' };
  const links = { repository: 'https://github.com/owner/app' };
  const repository = 'owner/app';
  const tag = 'v1.2.0-beta.1';
  const prepare = () => prepareRelease(folder, { product, links }, repository, tag);
  assert.throws(prepare, /Missing:/);
  for (const { name } of installerNames(product)) {
    writeFileSync(join(folder, name), 'installer');
  }
  writeFileSync(join(folder, 'extension.zip'), 'extension');
  assert.equal(JSON.parse(prepare().toString()).files.length, 8);
  assert.throws(
    () => prepareRelease(folder, { product, links }, 'owner/other', tag),
    /repository must match/,
  );
  assert.throws(
    () => prepareRelease(folder, { product, links }, repository, 'v1.2.0'),
    /tag.*does not match/,
  );
  writeFileSync(join(folder, 'App-1.2.0-beta.1-windows-arm64.rpm'), 'wrong format');
  assert.throws(prepare, /Unexpected: App-1.2.0-beta.1-windows-arm64.rpm/);
  writeFileSync(join(folder, 'Extra.EXE'), 'unexpected installer');
  assert.throws(prepare, /Extra\.EXE/);
});

test('the release commands prepare the files of GitHub and signed drafts', () => {
  const cwd = mkdtempSync(join(tmpdir(), 'wirecatch-release-'));
  const configDir = join(cwd, 'config');
  const release = join(cwd, 'release');
  const inventory = join(cwd, 'release-manifest.json');
  mkdirSync(configDir);
  mkdirSync(release);
  const config = readFileSync(join(ROOT, 'config', 'product.toml'), 'utf8');
  const { product, links } = readProduct(config);
  const repository = githubRepository(links.repository);
  const tag = `v${product.version}`;
  const changeMode = (settings) =>
    writeFileSync(join(configDir, 'product.toml'), withUpdateConfig(config, settings));
  for (const { name } of installerNames(product)) {
    writeFileSync(join(release, name), name);
  }
  const env = {
    ...process.env,
    GITHUB_REPOSITORY: repository,
    GITHUB_REF_NAME: tag,
    UPDATER_PRIVATE_KEY: '',
  };
  const run = (script, args, options) =>
    spawnSync(process.execPath, [join(ROOT, 'scripts', 'release', script), ...args], {
      cwd,
      env,
      encoding: 'utf8',
      ...options,
    });
  const draftFiles = (verification) =>
    prepareDraft({ inventory, folder: release, verification, tag })
      .files.map(({ name }) => name)
      .sort();
  const names = installerNames(product).map(({ name }) => name);

  changeMode('verification = "github"');
  const github = run('manifest.mjs', [release, inventory]);
  assert.equal(github.status, 0, github.stderr);
  assert.match(github.stdout, /^verification=github$/m);
  assert.equal(existsSync(join(release, MANIFEST_FILE)), false);
  assert.deepEqual(draftFiles('github'), [...names].sort());

  const { publicKey, privateKey } = generateKeyPairSync('ed25519');
  const encoded = publicKey
    .export({ type: 'spki', format: 'der' })
    .subarray(-32)
    .toString('base64url');
  changeMode(`verification = "signed"\npublic_key = "${encoded}"`);
  const signed = run('manifest.mjs', [release, inventory]);
  assert.equal(signed.status, 0, signed.stderr);
  assert.match(signed.stdout, /^verification=signed$/m);
  assert.ok(signed.stdout.includes(`public_key=${encoded}`));
  const signingEnv = {
    ...env,
    UPDATER_PRIVATE_KEY: privateKey.export({ type: 'pkcs8', format: 'der' }).toString('base64'),
    UPDATER_PUBLIC_KEY: encoded,
  };
  const refused = run('sign.mjs', [inventory, release], {
    env: { ...signingEnv, UPDATER_PUBLIC_KEY: 'wrong' },
  });
  assert.notEqual(refused.status, 0);
  assert.equal(existsSync(join(release, MANIFEST_FILE)), false);
  const signedFiles = run('sign.mjs', [inventory, release], { env: signingEnv });
  assert.equal(signedFiles.status, 0, signedFiles.stderr);
  assert.deepEqual(draftFiles('signed'), [...names, MANIFEST_FILE, SIGNATURE_FILE].sort());
  writeFileSync(join(release, MANIFEST_FILE), 'different signed inventory');
  assert.throws(() => draftFiles('signed'), /signed manifest differs/);
});
