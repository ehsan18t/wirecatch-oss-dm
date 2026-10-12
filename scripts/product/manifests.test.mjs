import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  editJson,
  fillTemplate,
  ManifestError,
  setPathDependencyVersions,
  setTomlKey,
  withNote,
} from './manifests.mjs';

const CARGO = `[workspace.package]
# The release.
version = "0.0.1"
edition = "2024"

[workspace.dependencies]
wirecatch-engine = { path = "crates/engine", version = "0.0.1" }
serde = { version = "1" }
`;

test('a Cargo setting is replaced in its own table and nowhere else', () => {
  const text = setTomlKey(CARGO, 'Cargo.toml', '[workspace.package]', ['version', '"0.2.0"']);
  assert.match(text, /# The release\.\nversion = "0\.2\.0"\nedition/);
  assert.match(text, /engine", version = "0\.0\.1"/);
});

test('a missing Cargo setting is added at the end of its table', () => {
  const text = setTomlKey(CARGO, 'Cargo.toml', '[workspace.package]', ['license', '"MIT"']);
  assert.match(text, /edition = "2024"\nlicense = "MIT"\n\n\[workspace\.dependencies\]/);
});

test('a table that is missing or repeated is an error, never a guess', () => {
  assert.throws(() => setTomlKey(CARGO, 'Cargo.toml', '[[bin]]', ['name', '"x"']), ManifestError);
  assert.throws(
    () => setTomlKey(`[[bin]]\nname = "a"\n[[bin]]\nname = "b"\n`, 'x', '[[bin]]', ['name', '"c"']),
    /exactly one \[\[bin\]\] table/,
  );
});

test("only the workspace's own crates take the product version", () => {
  const text = setPathDependencyVersions(CARGO, '[workspace.dependencies]', '0.2.0', 'Cargo.toml');
  assert.match(text, /engine", version = "0\.2\.0"/);
  assert.match(text, /serde = \{ version = "1" \}/);
});

test('a JSON edit keeps every other field', () => {
  const text = editJson('{"name":"a","version":"1"}', (json) => (json.version = '2'));
  assert.deepEqual(JSON.parse(text), { name: 'a', version: '2' });
});

test('a template takes values by name and refuses one that does not exist', () => {
  const product = { executables: { bridge: 'bridge' } };
  assert.equal(fillTemplate('run {{executables.bridge}} now', product, 't'), 'run bridge now');
  assert.throws(() => fillTemplate('{{executables.app}}', product, 't'), /executables\.app/);
});

test('the generated note never displaces a #! line', () => {
  assert.equal(withNote('#!/bin/sh\nexit 0\n', 'note', 'a.sh'), '#!/bin/sh\n# note\nexit 0\n');
  assert.equal(withNote('Name\n', 'note', 'a.nsh'), '; note\nName\n');
  assert.equal(withNote('# Title\n', 'note', 'a.md'), '<!-- note -->\n# Title\n');
});

test('an indented Cargo setting is replaced, never added a second time', () => {
  const text = setTomlKey('[[bin]]\n  name = "old"\n', 'x', '[[bin]]', ['name', '"new"']);
  assert.equal(text.match(/name =/g)?.length, 1);
  assert.match(text, /name = "new"/);
});

test('a malformed placeholder is an error, never left in the output', () => {
  const product = { executables: { bridge: 'bridge' } };
  for (const template of ['{{Executables.bridge}}', '{{ executables-bridge }}', 'x {{ y']) {
    assert.throws(() => fillTemplate(template, product, 't'), ManifestError, template);
  }
});
