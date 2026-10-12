import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { chromiumIdOf, placeholders, ProductError, readProduct } from './read.mjs';

const PRODUCT_TOML = readFileSync(
  join(import.meta.dirname, '..', '..', 'config', 'product.toml'),
  'utf8',
);

/** The repository's product.toml with one line replaced. */
function changed(from, to) {
  assert.ok(PRODUCT_TOML.includes(from), `product.toml has no ${from}`);
  return PRODUCT_TOML.replace(from, to);
}

function rejects(text, problem) {
  assert.throws(
    () => readProduct(text),
    (error) => error instanceof ProductError && problem.test(error.message),
  );
}

test("the repository's product identity is valid", () => {
  const product = readProduct(PRODUCT_TOML);
  assert.equal(product.product.name, 'Wirecatch');
  assert.equal(product.channels.release.name, 'Wirecatch');
  assert.equal(product.channels.dev.name, 'Wirecatch Dev');
});

test('the one Chromium id every bridge trusts follows from the extension key (D22)', () => {
  const { extension } = readProduct(PRODUCT_TOML);
  assert.equal(extension.chromium_id, 'chdgnmpjoipbgfeldicipccjjghmagki');
  assert.equal(chromiumIdOf(extension.chromium_public_key), extension.chromium_id);
});

test('an id that does not follow from the key is refused', () => {
  rejects(
    changed('chdgnmpjoipbgfeldicipccjjghmagki', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'),
    /does not follow from extension\.chromium_public_key/,
  );
});

test('a value of the wrong shape is refused, naming it', () => {
  rejects(changed('name = "app.wirecatch.bridge"', 'name = "App.Wirecatch"'), /native_host\.name/);
  rejects(changed('version = "0.1.0"', 'version = "one"'), /product\.version/);
  rejects(changed('socket = "app-dev.sock"', 'socket = "../app.sock"'), /channels\.dev\.socket/);
  rejects(
    changed('repository = "https://github.com', 'repository = "http://github.com'),
    /links\.repository/,
  );
});

test('an unknown or missing value is refused, so none is ever ignored', () => {
  rejects(changed('[files]\n', '[files]\nbackup = "x.db"\n'), /files\.backup is not a known value/);
  rejects(changed('database = "wirecatch.db"\n', ''), /files\.database must be a string/);
  rejects(`${PRODUCT_TOML}\n[store]\nurl = "x"\n`, /\[store\] is not a known section/);
});

test('two channels never share a folder, endpoint or identity', () => {
  rejects(changed('socket = "app-dev.sock"', 'socket = "app.sock"'), /own socket/);
  rejects(
    changed('linux_data_folder = "wirecatch-dev"', 'linux_data_folder = "wirecatch"'),
    /own linux_data_folder/,
  );
});

test('the values still to decide are found as placeholders', () => {
  assert.deepEqual(placeholders(readProduct(PRODUCT_TOML)), [
    'publisher.legal_name',
    'links.website',
    'contacts.support_email',
    'contacts.security_email',
  ]);
  const decided = changed(
    'legal_name = "TODO-PRODUCT-PUBLISHER-LEGAL-NAME"',
    'legal_name = "Example Ltd"',
  );
  assert.ok(!placeholders(readProduct(decided)).includes('publisher.legal_name'));
});

test('a value that would break the quoting of a shell or NSIS script is refused', () => {
  for (const name of ['Wire"catch', 'Wire$catch', 'Wire`catch', 'Wire\\catch']) {
    // A TOML literal string, so the value reaches the check as written.
    rejects(changed('name = "Wirecatch"', `name = '${name}'`), /product\.name/);
  }
  rejects(
    changed(
      'security_email = "TODO-PRODUCT-SECURITY-EMAIL@placeholder.invalid"',
      'security_email = "a$b@example.org"',
    ),
    /contacts\.security_email/,
  );
});

test('channels differing only in letter case still clash on Windows', () => {
  rejects(
    changed('windows_data_folder = "Wirecatch Dev"', 'windows_data_folder = "WIRECATCH"'),
    /own windows_data_folder/,
  );
  rejects(
    changed('pipe_prefix = "wirecatch-app-dev-"', 'pipe_prefix = "WIRECATCH-APP-"'),
    /own pipe_prefix/,
  );
  rejects(
    changed('identifier = "app.wirecatch.desktop.dev"', 'identifier = "app.Wirecatch.desktop"'),
    /identifier/,
  );
});

test('the app, the bridge and the CLI need executables of their own', () => {
  rejects(changed('cli = "wirecatch"', 'cli = "wirecatch-bridge"'), /executables/);
});

test('a folder or file name Windows cannot hold is refused', () => {
  for (const bad of ['wirecatch.', 'wirecatch ', 'CON', 'nul', 'aux.db', 'COM1', 'lpt9.txt']) {
    rejects(changed('database = "wirecatch.db"', `database = "${bad}"`), /files\.database/);
  }
});

test('a Firefox id longer than 255 characters is refused', () => {
  rejects(
    changed(
      'firefox_id = "extension@wirecatch.app"',
      `firefox_id = "${'a'.repeat(250)}@wirecatch.app"`,
    ),
    /extension\.firefox_id/,
  );
});
