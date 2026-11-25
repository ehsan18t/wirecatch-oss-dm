import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { candidateFolders, workspaceGlobs } from './workspace.mjs';

class Refused extends Error {}

test('the package globs are read, quoted or not, as folders of packages or single packages', () => {
  const manifest = 'packages:\n  - extension\n\n  - "packages/*"\nother: 1\n';
  assert.deepEqual(workspaceGlobs(manifest, Refused), [
    { folder: 'extension', nested: false },
    { folder: 'packages', nested: true },
  ]);
});

test('a manifest it cannot read is refused with the error asked for', () => {
  assert.throws(() => workspaceGlobs('other: 1\n', Refused), Refused);
  assert.throws(() => workspaceGlobs('packages:\n  - packages/**\n', Refused), Refused);
});

test('a single package is its own folder, and a nested glob gives each folder in it', () => {
  const root = mkdtempSync(join(tmpdir(), 'workspace-'));
  try {
    mkdirSync(join(root, 'packages', 'ui'), { recursive: true });
    mkdirSync(join(root, 'packages', 'units'));
    const globs = [
      { folder: 'extension', nested: false },
      { folder: 'packages', nested: true },
    ];
    assert.deepEqual(candidateFolders(root, globs).sort(), [
      'extension',
      'packages/ui',
      'packages/units',
    ]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
