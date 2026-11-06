import assert from 'node:assert/strict';
import { test } from 'node:test';
import { dialogsHeadedElsewhere } from './dialog-header.mjs';

const found = (text, headless = false) =>
  dialogsHeadedElsewhere(text, headless).map((hit) => `${String(hit.line)} ${hit.why}`);

test('a dialog titled inside its build is reported', () => {
  assert.deepEqual(
    found(
      'ui::dialog::open(window, cx, |_| "A".into(), |dialog, _, _| {\n    dialog.title("B")\n});',
    ),
    ['2 titles a dialog inside its build'],
  );
});

test('a dialog opened past ui::dialog is reported', () => {
  assert.deepEqual(found('window.open_dialog(cx, |dialog, _, _| dialog);'), [
    '1 opens a dialog past ui::dialog',
  ]);
  assert.deepEqual(found('window.open_alert_dialog(cx, |alert, _, _| alert);'), [
    '1 opens a dialog past ui::dialog',
  ]);
});

test('a dialog without its header is allowed only where named and in tests', () => {
  const headless = 'crate::ui::dialog::open_headless(window, cx, |dialog, _, _| dialog);';
  assert.deepEqual(found(headless), ['1 opens a dialog without its header']);
  assert.deepEqual(found(headless, true), []);
  assert.deepEqual(found(`fn real() {}\n#[cfg(test)]\nmod tests {\n    ${headless}\n}`), []);
});

test('titles outside a dialog build are left alone', () => {
  assert.deepEqual(
    found(
      'SettingGroup::new().title("Video");\nui::dialog::open(window, cx, |_| "Remove?".into(), |dialog, _, _| dialog.w(px(440.)));',
    ),
    [],
  );
  assert.deepEqual(found('// dialog.title("A") in a comment\nlet x = 1;'), []);
});
