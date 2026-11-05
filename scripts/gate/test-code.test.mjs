import assert from 'node:assert/strict';
import { test } from 'node:test';
import { codeOnly } from './rust-attributes.mjs';
import { isTestFile, testItems, testModuleFolders } from './test-code.mjs';

/** The text of each test item in `text`. */
const items = (...text) => {
  const code = codeOnly(text.join('\n'));
  return testItems(code).map(([start, end]) => code.slice(start, end));
};

test('an item ends past its body, whatever comes after', () => {
  assert.deepEqual(items('#[test]', 'fn a() {}', 'fn b() {}'), ['#[test]\nfn a() {}']);
});

test('an item with a semicolon in its type ends at its own semicolon', () => {
  assert.deepEqual(items('#[cfg(test)]', 'const A: [u8; 1] = [1];', 'const B: u8 = 2;'), [
    '#[cfg(test)]\nconst A: [u8; 1] = [1];',
  ]);
});

test('a field only tests build does not reach past its struct', () => {
  assert.deepEqual(items('struct S {', '    #[cfg(test)]', '    a: u8', '}', 'fn b() {}'), [
    '#[cfg(test)]\n    a: u8\n',
  ]);
});

test('a parameter only tests build does not reach past its parameter list', () => {
  assert.deepEqual(items('fn a(#[cfg(test)] h: u8) {', '    real();', '}'), ['#[cfg(test)] h: u8']);
  assert.deepEqual(items('fn a(#[cfg(test)] h: u8, b: u8) {', '    real();', '}'), [
    '#[cfg(test)] h: u8,',
  ]);
  assert.deepEqual(items('let a = [#[cfg(test)] 1];', 'real();'), ['#[cfg(test)] 1']);
});

test('a field or variant only tests build ends at its comma', () => {
  assert.deepEqual(items('struct S {', '    #[cfg(test)]', '    a: u8,', '    b: u8,', '}'), [
    '#[cfg(test)]\n    a: u8,',
  ]);
  assert.deepEqual(
    items('struct S {', '    #[cfg(test)]', '    pub a: (u8, u8),', '    b: u8,', '}'),
    ['#[cfg(test)]\n    pub a: (u8, u8),'],
  );
  assert.deepEqual(items('enum E {', '    #[cfg(test)]', '    A(u8, u8),', '    B,', '}'), [
    '#[cfg(test)]\n    A(u8, u8),',
  ]);
});

test('a comma in an item with generics does not end it', () => {
  for (const item of [
    'fn a<T, U>() {}',
    'pub(crate) fn a<T, U>() {}',
    'struct S<T, U> {}',
    'impl<T, U> S<T, U> {}',
    'const A: M<u8, u8> = b();',
    'static A: M<u8, u8> = b();',
    'type A = M<u8, u8>;',
    'use a::{b, c}',
    'let a: M<u8, u8> = b();',
    '#[inline] fn a<T, U>() {}',
  ]) {
    assert.deepEqual(items('#[test]', item, 'fn real() {}'), [`#[test]\n${item}`], item);
  }
});

test("a cfg a person's build can hold is not a test item", () => {
  for (const attribute of [
    '#[cfg(unix)]',
    '#[cfg(not(test))]',
    '#[cfg(any(unix, test))]',
    '#[cfg(feature = "x")]',
    '#[cfg(all(unix, not(test)))]',
  ]) {
    assert.deepEqual(items(attribute, 'fn a() {}'), [], attribute);
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
  assert.ok(isTestFile('crates/a/src/fake.rs', under));
  assert.ok(isTestFile('crates/a/src/fake/mod.rs', under));
  assert.ok(isTestFile('crates/a/src/fake/deep.rs', under));
  assert.ok(!isTestFile('crates/a/src/fakes.rs', under));
});

test('integration tests and a tests.rs file are test code', () => {
  assert.ok(isTestFile('crates/a/tests/run.rs', []));
  assert.ok(isTestFile('crates/a/src/x/tests.rs', []));
  assert.ok(!isTestFile('crates/a/src/latests.rs', []));
});
