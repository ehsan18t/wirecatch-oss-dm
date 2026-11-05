import assert from 'node:assert/strict';
import { test } from 'node:test';
import { namesInStrings } from './product-name.mjs';

const lines = (...text) => namesInStrings(text.join('\n'), 'Wirecatch');

test('the name in a string is found on its line', () => {
  assert.deepEqual(lines('fn a() {', '    say("Wirecatch stopped");', '}'), [2]);
});

test('a raw string counts too', () => {
  assert.deepEqual(lines('const A: &str = r#"Wirecatch "said" so"#;'), [1]);
});

test('comments, names in code and a longer word do not count', () => {
  assert.deepEqual(
    lines('// Wirecatch does this', 'struct Wirecatch;', 'const A: &str = "WirecatchBridgeTest";'),
    [],
  );
});

test('a test module does not count, however it is written', () => {
  for (const attribute of ['#[cfg(test)]', '#[cfg(all(test, unix))]']) {
    for (const module of ['mod tests', 'pub(crate) mod tests']) {
      assert.deepEqual(lines(attribute, `${module} {`, '    say("Wirecatch");', '}'), []);
    }
  }
});

test('a string after a test module counts', () => {
  assert.deepEqual(lines('#[cfg(test)]', 'mod tests {', '}', 'const A: &str = "Wirecatch";'), [4]);
});
