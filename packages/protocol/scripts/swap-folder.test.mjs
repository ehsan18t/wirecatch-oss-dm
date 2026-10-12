import assert from 'node:assert/strict';
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  renameSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, beforeEach, test } from 'node:test';
import { nextFolder, recoverInterruptedSwap, removePrevious, swapIn } from './swap-folder.mjs';

let root;
let folder;

/** A folder holding one file with `text`. */
function folderWith(path, text) {
  mkdirSync(path);
  writeFileSync(join(path, 'A.ts'), text);
}

const contents = (path) => readFileSync(join(path, 'A.ts'), 'utf8');

beforeEach(() => {
  root = mkdtempSync(join(tmpdir(), 'swap-folder-'));
  folder = join(root, 'generated');
  folderWith(folder, 'old');
  folderWith(nextFolder(folder), 'new');
});

afterEach(() => rmSync(root, { recursive: true, force: true }));

test('the new folder replaces the old one', () => {
  swapIn(folder);
  removePrevious(folder);
  assert.equal(contents(folder), 'new');
  assert.equal(existsSync(nextFolder(folder)), false);
  assert.equal(existsSync(`${folder}.previous`), false);
});

test('a locked folder is reported in plain words and the old folder stays in place', () => {
  // The second rename fails as Windows fails it when another program has the folder open.
  let renames = 0;
  const rename = (from, to) => {
    renames += 1;
    if (renames === 2) throw Object.assign(new Error('operation not permitted'), { code: 'EPERM' });
    renameSync(from, to);
  };
  assert.throws(() => swapIn(folder, { rename }), /open in another program \(EPERM\)/);
  assert.equal(contents(folder), 'old');
  assert.equal(existsSync(`${folder}.previous`), false);
});

test('a run stopped between the two renames is undone by the next run', () => {
  renameSync(folder, `${folder}.previous`);
  recoverInterruptedSwap(folder);
  assert.equal(contents(folder), 'old');
});

test('a failed undo says where the old files are, and the next run puts them back', () => {
  let renames = 0;
  const rename = (from, to) => {
    renames += 1;
    if (renames >= 2) throw Object.assign(new Error('resource busy'), { code: 'EBUSY' });
    renameSync(from, to);
  };
  assert.throws(
    () => swapIn(folder, { rename }),
    /They are in .*generated\.previous; the next run/,
  );
  recoverInterruptedSwap(folder);
  assert.equal(contents(folder), 'old');
});

test('an interrupted run that left both folders keeps the current one', () => {
  folderWith(`${folder}.previous`, 'older');
  recoverInterruptedSwap(folder);
  assert.equal(contents(folder), 'old');
  swapIn(folder);
  removePrevious(folder);
  assert.equal(contents(folder), 'new');
  assert.equal(existsSync(`${folder}.previous`), false);
});

test('any other failure keeps its own message', () => {
  rmSync(nextFolder(folder), { recursive: true });
  assert.throws(() => swapIn(folder), /ENOENT/);
  assert.equal(contents(folder), 'old');
});
