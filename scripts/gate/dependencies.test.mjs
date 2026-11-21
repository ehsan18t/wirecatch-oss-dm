import assert from 'node:assert/strict';
import { test } from 'node:test';
import { dependencyProblems } from './dependencies.mjs';

test('reports direct shared dependencies, aliases and workspace overrides', () => {
  const workspace = '[workspace.dependencies]\nshared = "1"';
  const manifests = new Map([
    ['a/Cargo.toml', '[dependencies]\nrenamed = { package = "actual", version = "1" }'],
    ['b/Cargo.toml', '[target."cfg(unix)".build-dependencies]\nactual = "1"'],
    ['c/Cargo.toml', '[dev-dependencies.shared]\nversion = "1"'],
    [
      'd/Cargo.toml',
      '[dependencies]\nshared = { workspace = true }\nlocal = { path = "../local" }',
    ],
  ]);
  assert.deepEqual(dependencyProblems(workspace, manifests), [
    'a/Cargo.toml:2 (renamed)',
    'b/Cargo.toml:2 (actual)',
    'c/Cargo.toml:1 (shared)',
  ]);
});

test('accepts inherited and single-crate dependencies', () => {
  const workspace = '[workspace.dependencies]\nshared = "1"';
  const manifests = new Map([
    ['a/Cargo.toml', '[dependencies]\nshared.workspace = true\nunique = "1"'],
    ['b/Cargo.toml', '[dev-dependencies]\nshared = { workspace = true, features = ["extra"] }'],
    [
      'c/Cargo.toml',
      '[dependencies]\nalias = { package = "unique", git = "https://example.com/repo" }',
    ],
  ]);
  assert.deepEqual(dependencyProblems(workspace, manifests), []);
});
