import assert from 'node:assert/strict';
import { test } from 'node:test';
import { discardedResults } from './discarded.mjs';

const lines = (text) => discardedResults(text).map((hit) => hit.line);

test('a result dropped with `_ =` is reported', () => {
  assert.deepEqual(lines('fn f() {\n    _ = send(x);\n}'), [2]);
  assert.deepEqual(
    lines('    _ = form.update(cx, |form, cx| {\n        cx.notify();\n    });'),
    [1],
  );
  assert.deepEqual(lines('let f = || {\n    _ = done.send(())\n};'), [2]);
  assert.deepEqual(
    lines(
      '_ = view.update(cx, |view, cx| match view.state {\n    Some(x) => x,\n    None => 0,\n});',
    ),
    [1],
  );
});

test('a result dropped inside a line is reported', () => {
  assert.deepEqual(lines('match x {\n    Some(x) => _ = tx.send(x),\n    None => {}\n}'), [2]);
  assert.deepEqual(lines('match x {\n    Some(x) =>\n        _ = tx.send(x),\n}'), [3]);
  assert.deepEqual(lines('xs.iter().for_each(|x| _ = tx.send(x));'), [1]);
  assert.deepEqual(lines('if ok { _ = tx.send(x); }'), [1]);
  assert.deepEqual(lines('a(); _ = tx.send(x);'), [1]);
});

test('a pattern or binding named `_` is not', () => {
  assert.deepEqual(lines('let _ = keep;\nlet (a, _) = pair;\n(_, b) = pair;'), []);
  assert.deepEqual(lines('select! { biased; _ = stop.cancelled() => {}, _ = tick => {} }'), []);
});

test('a select arm, a comparison and text are not', () => {
  assert.deepEqual(
    lines('tokio::select! {\n    _ = tick.tick() => {}\n    _ = stop => break,\n}'),
    [],
  );
  assert.deepEqual(lines('select! {\n    _ = &mut from_app => {\n        x();\n    }\n}'), []);
  assert.deepEqual(lines('let same = a\n    _ == b;'), []);
  assert.deepEqual(lines('match x {\n    Some(y) => y,\n    _ => {}\n}'), []);
  assert.deepEqual(lines('// _ = send(x);\nlet text = "\n_ = send(x);\n";'), []);
  assert.deepEqual(lines('let (Ok(()) | Err(_)) = sent;'), []);
});
