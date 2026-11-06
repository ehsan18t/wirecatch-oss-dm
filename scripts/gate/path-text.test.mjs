import assert from 'node:assert/strict';
import { test } from 'node:test';
import { pathsCutAtTheEnd } from './path-text.mjs';

const lines = (text) => pathsCutAtTheEnd(text).map((hit) => hit.line);

test('a path cut at its end is reported', () => {
  assert.deepEqual(lines('div().truncate().child(row.path.clone())'), [1]);
  assert.deepEqual(lines('div().min_w_0().truncate().child(folder)'), [1]);
  assert.deepEqual(lines('div().child(download_dir)\n    .truncate()'), [2]);
  assert.deepEqual(lines('div().truncate().child(saved.display().to_string())'), [1]);
  assert.deepEqual(
    lines('.children(paths.iter().map(|shown| div().truncate().child(shown.to_string_lossy())))'),
    [1],
  );
});

test('text that is not a path may be cut at its end', () => {
  assert.deepEqual(lines('div().truncate().child(row.file_name.clone())'), []);
  assert.deepEqual(lines('div().truncate().child(link_path(&link).to_owned())'), []);
  assert.deepEqual(lines('div().truncate().child("Choose a folder")'), []);
  assert.deepEqual(lines('div().truncate().child(name) // the path stays whole'), []);
  assert.deepEqual(lines('rows.truncate(path_count)'), []);
});

test('only the chain that is cut is read', () => {
  assert.deepEqual(lines('h_flex().child(div().truncate().child(name)).child(folder)'), []);
  assert.deepEqual(lines('h_flex()\n    .child(PathText::new(folder))\n    .truncate()'), []);
  assert.deepEqual(lines('let shown = div().truncate().child(title);\nlet path = folder;'), []);
});
