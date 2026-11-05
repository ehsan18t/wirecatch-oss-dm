import assert from 'node:assert/strict';
import { test } from 'node:test';
import { itemsWithoutCallers, productCode, publicItems } from './shell-callers.mjs';

const SHELL = 'crates/shell/src/api.rs';
const DESKTOP = 'crates/desktop/src/main.rs';

const unused = (shell, desktop = '') =>
  itemsWithoutCallers(
    new Map([
      [SHELL, productCode(shell)],
      [DESKTOP, productCode(desktop)],
    ]),
  );

test('every kind of public item is listed, and only public ones', () => {
  const code = `
pub fn one() {}
pub async fn two() {}
pub const fn three() {}
pub struct Four;
pub enum Five {}
pub trait Six {}
pub type Seven = u8;
pub const EIGHT: u8 = 8;
pub static NINE: u8 = 9;
pub mod ten;
impl Four { pub fn eleven(&self) {} }
    pub fn twelve() {}
fn private() {}
pub(crate) fn inside() {}
`;
  assert.deepEqual(publicItems(code).slice(0, 10), [
    'one',
    'two',
    'three',
    'Four',
    'Five',
    'Six',
    'Seven',
    'EIGHT',
    'NINE',
    'ten',
  ]);
  assert.ok(publicItems(code).includes('twelve'));
  assert.ok(!publicItems(code).includes('private'));
  assert.ok(!publicItems(code).includes('inside'));
});

test('an item nothing names is reported', () => {
  assert.deepEqual(unused('pub fn orphan() {}\n', 'fn main() {}'), [`${SHELL}: orphan`]);
});

test('an item another crate names has a caller', () => {
  assert.deepEqual(unused('pub fn called() {}\n', 'fn main() { shell::called(); }'), []);
});

test('the same name for each system counts as one item', () => {
  const code = '#[cfg(windows)]\npub fn on() {}\n#[cfg(unix)]\npub fn on() {}\n';
  assert.equal(unused(code).length, 2);
  assert.deepEqual(unused(code, 'fn main() { on(); }'), []);
});

test('a mention in a comment is not a caller', () => {
  assert.equal(unused('pub fn orphan() {}\n', '// orphan does this\n').length, 1);
});

test('a test module is not a caller', () => {
  const shell = `pub fn orphan() {}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        super::orphan();
    }
}
`;
  assert.equal(unused(shell).length, 1);
});

test('code after a test module still counts', () => {
  const shell = `pub fn called() {}

#[cfg(test)]
mod tests {
}

fn after() { called(); }
`;
  assert.deepEqual(unused(shell), []);
});
