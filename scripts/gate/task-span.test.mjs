import assert from 'node:assert/strict';
import { test } from 'node:test';
import { unwrappedSpawns } from './task-span.mjs';

const lines = (text) => unwrappedSpawns(text).map((hit) => hit.line);

test("tokio's own spawns in shipped code are reported", () => {
  assert.deepEqual(lines('fn f() {\n    tokio::spawn(work());\n}'), [2]);
  assert.deepEqual(lines('let t = tokio::task::spawn_blocking(move || read());'), [1]);
  assert.deepEqual(lines('let t = tokio::task::spawn(async {});'), [1]);
  assert.deepEqual(lines('self.inner.runtime.spawn_blocking(move || work());'), [1]);
  assert.deepEqual(lines('runtime\n    .spawn(task)'), [1]);
  assert.deepEqual(lines('Handle::current().spawn(task);'), [1]);
});

test('the wrapper, a JoinSet and code only tests build are not', () => {
  assert.deepEqual(lines('let t = wirecatch_engine::task::spawn(work());'), []);
  assert.deepEqual(lines('set.spawn(worker());'), []);
  assert.deepEqual(lines('#[cfg(test)]\nmod tests {\n    fn f() { tokio::spawn(x()); }\n}'), []);
  assert.deepEqual(lines('#[tokio::test]\nasync fn t() {\n    tokio::spawn(x());\n}'), []);
  assert.deepEqual(lines('// tokio::spawn(x());\nlet s = "tokio::spawn(";'), []);
});
