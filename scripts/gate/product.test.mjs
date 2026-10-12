import assert from 'node:assert/strict';
import { cpSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { after, before, test } from 'node:test';
import { generate } from '../product/generate.mjs';
import { outputs } from '../product/outputs.mjs';
import {
  crateMetadataProblems,
  handCopiedPlaceholders,
  remainingPlaceholders,
  staleOutputs,
  placeholderHits,
} from './product.mjs';
import { ROOT } from './process.mjs';

/** What the generator reads besides its outputs: the identity, the templates, and the formatting settings. */
const INPUTS = ['config', 'rustfmt.toml', '.prettierrc.json', 'pnpm-workspace.yaml', 'Cargo.lock'];

let root;

before(() => {
  root = mkdtempSync(join(tmpdir(), 'product-check-'));
  for (const path of [...INPUTS, ...outputs(ROOT).map((output) => output.path)]) {
    cpSync(join(ROOT, path), join(root, path), { recursive: true });
  }
});

after(() => rmSync(root, { recursive: true, force: true }));

const edit = (path, change) =>
  writeFileSync(join(root, path), change(readFileSync(join(root, path), 'utf8')));

test('a placeholder copied by hand outside the generated files is found', () => {
  assert.deepEqual(
    handCopiedPlaceholders(['src/a.rs:3:TODO-PRODUCT-WEBSITE', 'SECURITY.md:8:x'], ['SECURITY.md']),
    ['src/a.rs:3:TODO-PRODUCT-WEBSITE'],
  );
});

test('the repository holds placeholders only where config/product.toml puts them', () => {
  assert.deepEqual(
    handCopiedPlaceholders(
      placeholderHits(),
      outputs(ROOT).map((o) => o.path),
    ),
    [],
  );
});

test('the generated files in the repository are current', async () => {
  assert.deepEqual(await staleOutputs(ROOT), []);
});

test('a generated file edited by hand, or missing, is reported until generated again', async () => {
  edit('packages/product/src/index.ts', (text) => text.replace("'Wirecatch'", "'Other'"));
  rmSync(join(root, 'apps/desktop/src-tauri/linux/prerm.sh'));
  assert.deepEqual(await staleOutputs(root), [
    'packages/product/src/index.ts',
    'apps/desktop/src-tauri/linux/prerm.sh',
  ]);
  await generate(root);
  assert.deepEqual(await staleOutputs(root), []);
});

test('a Cargo.lock that still records another version of a workspace crate is stale', async () => {
  edit('Cargo.lock', (text) =>
    text.replace(
      'name = "wirecatch-ipc"\nversion = "0.1.0"',
      'name = "wirecatch-ipc"\nversion = "0.0.9"',
    ),
  );
  assert.deepEqual(await staleOutputs(root), ['Cargo.lock']);
  edit('Cargo.lock', (text) => text.replace('version = "0.0.9"', 'version = "0.1.0"'));
});

test('a crate that writes its own authors, repository or homepage is reported', async () => {
  assert.deepEqual(crateMetadataProblems(root), []);
  edit('crates/cli/Cargo.toml', (text) =>
    text.replace('authors.workspace = true', 'authors = ["Someone"]'),
  );
  assert.match(crateMetadataProblems(root).join('\n'), /crates\/cli\/Cargo\.toml.*authors/);
  edit('crates/cli/Cargo.toml', (text) =>
    text.replace('authors = ["Someone"]', 'authors.workspace = true'),
  );
});

test('a change to product.toml makes every file that carries the value stale', async () => {
  edit('config/product.toml', (text) =>
    text.replace('bridge = "wirecatch-bridge"', 'bridge = "wc-bridge"'),
  );
  const stale = await staleOutputs(root);
  for (const path of [
    'crates/product/src/lib.rs',
    'apps/desktop/src-tauri/linux/prerm.sh',
    'apps/desktop/src-tauri/windows/hooks.nsh',
    'crates/bridge/Cargo.toml',
    'apps/desktop/src-tauri/tauri.bundle.conf.json',
  ]) {
    assert.ok(stale.includes(path), `${path} should be stale: ${stale.join(', ')}`);
  }
  await generate(root);
  assert.match(readFileSync(join(root, 'crates/bridge/Cargo.toml'), 'utf8'), /name = "wc-bridge"/);
});

test('the release check finds every placeholder until the values are decided', async () => {
  const before = await remainingPlaceholders(root);
  assert.equal(before.values.length, 4);
  assert.ok(before.lines.some((line) => line.startsWith('SECURITY.md:')));
  edit('config/product.toml', (text) =>
    text
      .replace('"TODO-PRODUCT-PUBLISHER-LEGAL-NAME"', '"Example Ltd"')
      .replace('"https://TODO-PRODUCT-WEBSITE.invalid/"', '"https://example.com/"')
      .replace('"TODO-PRODUCT-SUPPORT-EMAIL@placeholder.invalid"', '"help@example.com"')
      .replace('"TODO-PRODUCT-SECURITY-EMAIL@placeholder.invalid"', '"security@example.com"'),
  );
  await generate(root);
  assert.deepEqual(await remainingPlaceholders(root), { values: [], lines: [] });
});

test('a workspace package whose package.json was deleted is reported, not skipped', async () => {
  rmSync(join(root, 'apps/extension/package.json'));
  await assert.rejects(staleOutputs(root), /apps\/extension\/package\.json is missing/);
});
