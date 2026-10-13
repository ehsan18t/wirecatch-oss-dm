import assert from 'node:assert/strict';
import { test } from 'node:test';
import { changedSnapshots } from './javascript.mjs';

test('a released snapshot that was edited or removed is reported, one kept as it was is not', () => {
  const there = { 'v1/Call.ts': 'a', 'v1/Event.ts': 'b', 'v1/ErrorCode.ts': 'c' };
  const here = { 'v1/Call.ts': 'a', 'v1/Event.ts': 'b2' };
  assert.deepEqual(
    changedSnapshots(
      Object.keys(there),
      (path) => there[path],
      (path) => here[path] ?? null,
    ),
    ['v1/Event.ts', 'v1/ErrorCode.ts'],
  );
});
