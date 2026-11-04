import assert from 'node:assert/strict';
import { test } from 'node:test';
import { workspaceParents } from './workspace.mjs';

class Refused extends Error {}

test('the folders of the package globs are read, quoted or not', () => {
  const manifest = 'packages:\n  - apps/*\n\n  - "packages/*"\nother: 1\n';
  assert.deepEqual(workspaceParents(manifest, Refused), ['apps', 'packages']);
});

test('a manifest it cannot read is refused with the error asked for', () => {
  assert.throws(() => workspaceParents('other: 1\n', Refused), Refused);
  assert.throws(() => workspaceParents('packages:\n  - apps/**\n', Refused), Refused);
});
