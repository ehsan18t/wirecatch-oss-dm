import assert from 'node:assert/strict';
import { cpSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { after, before, test } from 'node:test';
import { generate } from '../product/generate.mjs';
import { outputs } from '../product/outputs.mjs';
import { remainingPlaceholders, staleOutputs } from './product.mjs';
import { ROOT } from './process.mjs';

/** What the generator reads besides its outputs: the identity, the templates, and the formatting settings. */
const INPUTS = ['config', 'rustfmt.toml', '.prettierrc.json'];

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

test('a change to product.toml makes every file that carries the value stale', async () => {
  edit('config/product.toml', (text) =>
    text.replace('bridge = "wirecatch-bridge"', 'bridge = "wc-bridge"'),
  );
  const stale = await staleOutputs(root);
  for (const path of [
    'crates/product/src/lib.rs',
    'packages/product/src/index.ts',
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
