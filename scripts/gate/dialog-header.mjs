// `dialog-header`: every dialog of the desktop app is headed by `ui::dialog::open`, which draws the one header from the title it is given (D260). A dialog opened another way, or titled inside its build, would head itself.
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { filesMatching, GateError, ROOT } from './process.mjs';
import { closingBracket, codeOnly } from './rust-attributes.mjs';

const SOURCES = 'crates/desktop/src/*.rs';
const DIALOG = 'crates/desktop/src/ui/dialog.rs';
/** The dialogs that draw their own heading or none (D260.1). */
const HEADLESS = new Set([
  'crates/desktop/src/main_window/palette.rs',
  'crates/desktop/src/main_window/details.rs',
]);

const line = (code, index) => code.slice(0, index).split('\n').length;

/** The places in one Rust file that head a dialog another way, as `{ line, why }`; `headless` says the file may open one without a header. */
export function dialogsHeadedElsewhere(text, headless) {
  const code = codeOnly(text);
  const tests = code.search(/#\[cfg\(test\)\]/);
  const found = [];
  for (const match of code.matchAll(/\b(?:open_dialog|open_alert_dialog)\s*\(/g)) {
    found.push({ line: line(code, match.index), why: 'opens a dialog past ui::dialog' });
  }
  for (const match of code.matchAll(/\bopen_headless\s*\(/g)) {
    if (!headless && (tests === -1 || match.index < tests)) {
      found.push({ line: line(code, match.index), why: 'opens a dialog without its header' });
    }
  }
  for (const match of code.matchAll(/\bdialog::open\s*\(/g)) {
    const open = match.index + match[0].length - 1;
    const call = code.slice(open, closingBracket(code, open));
    const title = call.search(/\.\s*title\s*\(/);
    if (title !== -1) {
      found.push({ line: line(code, open + title), why: 'titles a dialog inside its build' });
    }
  }
  return found;
}

function dialogHeader() {
  const hits = filesMatching('open_dialog|open_alert_dialog|open_headless|dialog::open', [
    SOURCES,
    `:!${DIALOG}`,
  ]).flatMap((file) =>
    dialogsHeadedElsewhere(readFileSync(join(ROOT, file), 'utf8'), HEADLESS.has(file)).map(
      (hit) => `${file}:${String(hit.line)}: ${hit.why}`,
    ),
  );
  if (hits.length > 0) {
    throw new GateError(
      `Dialogs headed another way: open each through ui::dialog::open with its title, which draws the one header (D260):\n${hits.join('\n')}`,
    );
  }
}

export const DIALOG_HEADER_CHECKS = { 'dialog-header': dialogHeader };
