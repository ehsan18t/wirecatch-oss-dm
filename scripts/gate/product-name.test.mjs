import assert from 'node:assert/strict';
import { test } from 'node:test';
import { isTestModuleFile, namesInStrings, testModuleFolders } from './product-name.mjs';

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

test('a single item only tests build does not count', () => {
  for (const attribute of [
    '#[cfg(test)]',
    '#[test]',
    '#[tokio::test]',
    '#[tokio::test(flavor = "multi_thread")]',
    '#[cfg(any(test, feature = "x"))]',
    '#[cfg(all(unix, test))]',
  ]) {
    assert.deepEqual(lines(attribute, 'fn h() {', '    say("Wirecatch");', '}'), [], attribute);
  }
});

test('an item with a semicolon in its type ends at its own semicolon', () => {
  assert.deepEqual(
    lines('#[cfg(test)]', 'const A: [&str; 1] = ["Wirecatch"];', 'const B: &str = "Wirecatch";'),
    [3],
  );
});

test("code a person's build can run still counts", () => {
  for (const attribute of [
    '#[cfg(unix)]',
    '#[cfg(not(test))]',
    '#[cfg(any(unix, test))]',
    '#[cfg(feature = "x")]',
    '#[derive(Debug)]',
  ]) {
    assert.deepEqual(lines(attribute, 'fn h() {', '    say("Wirecatch");', '}'), [3], attribute);
  }
});

test('a file behind a test-only module declaration is test code', () => {
  const folders = (file, ...text) => testModuleFolders(file, text.join('\n'));
  assert.deepEqual(folders('crates/a/src/lib.rs', '#[cfg(test)]', 'mod fake;', 'mod real;'), [
    'crates/a/src/fake',
  ]);
  assert.deepEqual(folders('crates/a/src/x/mod.rs', '#[cfg(test)]', 'pub(crate) mod fake;'), [
    'crates/a/src/x/fake',
  ]);
  assert.deepEqual(folders('crates/a/src/x.rs', '#[cfg(test)]', 'mod fake;'), [
    'crates/a/src/x/fake',
  ]);
  const under = ['crates/a/src/fake'];
  assert.ok(isTestModuleFile('crates/a/src/fake.rs', under));
  assert.ok(isTestModuleFile('crates/a/src/fake/mod.rs', under));
  assert.ok(isTestModuleFile('crates/a/src/fake/deep.rs', under));
  assert.ok(!isTestModuleFile('crates/a/src/fakes.rs', under));
});
