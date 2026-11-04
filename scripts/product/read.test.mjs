import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { UNDECIDED_VALUES } from './identity-samples.mjs';
import { setTomlKey, tomlString } from './manifests.mjs';
import { chromiumIdOf, placeholders, ProductError, readProduct } from './read.mjs';

const PRODUCT_TOML = readFileSync(
  join(import.meta.dirname, '..', '..', 'config', 'product.toml'),
  'utf8',
);
const PRODUCT = readProduct(PRODUCT_TOML);

/** The repository's product.toml with one value set, whatever it holds today. */
const withValue = (table, key, value) =>
  setTomlKey(PRODUCT_TOML, 'product.toml', `[${table}]`, [key, tomlString(value)]);

function rejects(text, problem) {
  assert.throws(
    () => readProduct(text),
    (error) => error instanceof ProductError && problem.test(error.message),
  );
}

test("the repository's product identity is valid", () => {
  assert.equal(PRODUCT.channels.release.name, PRODUCT.product.name);
  assert.notEqual(PRODUCT.channels.dev.name, PRODUCT.channels.release.name);
});

test('the one Chromium id every bridge trusts follows from the extension key (D22)', () => {
  const { extension } = PRODUCT;
  assert.match(extension.chromium_id, /^[a-p]{32}$/);
  assert.equal(chromiumIdOf(extension.chromium_public_key), extension.chromium_id);
});

test('an id that does not follow from the key is refused', () => {
  rejects(
    withValue('extension', 'chromium_id', 'a'.repeat(32).replace(/^a/, 'b')),
    /does not follow from extension\.chromium_public_key/,
  );
});

test('a value of the wrong shape is refused, naming it', () => {
  rejects(withValue('native_host', 'name', 'App.Wirecatch'), /native_host\.name/);
  rejects(withValue('product', 'version', 'one'), /product\.version/);
  rejects(withValue('channels.dev', 'socket', '../app.sock'), /channels\.dev\.socket/);
  rejects(withValue('links', 'repository', 'http://example.com/a'), /links\.repository/);
});

test('an unknown or missing value is refused, so none is ever ignored', () => {
  rejects(
    PRODUCT_TOML.replace('[files]\n', '[files]\nbackup = "x.db"\n'),
    /files\.backup is not a known value/,
  );
  rejects(PRODUCT_TOML.replace(/^database = .*\n/m, ''), /files\.database must be a string/);
  rejects(`${PRODUCT_TOML}\n[store]\nurl = "x"\n`, /\[store\] is not a known section/);
});

test('two channels never share a folder, endpoint or identity', () => {
  const { release } = PRODUCT.channels;
  rejects(withValue('channels.dev', 'socket', release.socket), /own socket/);
  rejects(
    withValue('channels.dev', 'linux_data_folder', release.linux_data_folder),
    /own linux_data_folder/,
  );
});

test('a value still to decide is found as a placeholder until it is decided', () => {
  for (const [table, key, placeholder, decided] of UNDECIDED_VALUES) {
    const name = `${table}.${key}`;
    assert.ok(placeholders(readProduct(withValue(table, key, placeholder))).includes(name), name);
    assert.ok(!placeholders(readProduct(withValue(table, key, decided))).includes(name), name);
  }
});

test('a value that would break the quoting of a shell script is refused', () => {
  for (const name of ['Wire"catch', 'Wire$catch', 'Wire`catch', String.raw`Wire\catch`]) {
    rejects(withValue('product', 'name', name), /product\.name/);
  }
  rejects(withValue('contacts', 'security_email', 'a$b@example.org'), /contacts\.security_email/);
});

test('channels differing only in letter case still clash on Windows', () => {
  const { release } = PRODUCT.channels;
  rejects(
    withValue('channels.dev', 'windows_data_folder', release.windows_data_folder.toUpperCase()),
    /own windows_data_folder/,
  );
  rejects(
    withValue('channels.dev', 'pipe_prefix', release.pipe_prefix.toUpperCase()),
    /own pipe_prefix/,
  );
  rejects(withValue('channels.dev', 'identifier', 'app.Wirecatch.desktop'), /identifier/);
});

test('the app, the bridge and the CLI need executables of their own', () => {
  rejects(withValue('executables', 'cli', PRODUCT.executables.bridge), /executables/);
});

test('a folder or file name Windows cannot hold is refused', () => {
  for (const bad of ['wirecatch.', 'wirecatch ', 'CON', 'nul', 'aux.db', 'COM1', 'lpt9.txt']) {
    rejects(withValue('files', 'database', bad), /files\.database/);
  }
});

test('a Firefox id longer than 255 characters is refused', () => {
  rejects(
    withValue('extension', 'firefox_id', `${'a'.repeat(250)}@example.org`),
    /extension\.firefox_id/,
  );
});
