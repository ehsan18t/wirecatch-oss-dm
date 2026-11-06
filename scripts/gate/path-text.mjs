// `path-text`: a path the desktop app shows is cut in its middle by `ui::PathText`, never at its end by `.truncate()`, which hides the last folder or file name (D239). Paths are told apart by name, since a text check cannot see types.
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { filesMatching, GateError, ROOT } from './process.mjs';
import { closingBracket, codeOnly } from './rust-attributes.mjs';

const SOURCES = 'crates/desktop/src/*.rs';
const PATH_TEXT = 'crates/desktop/src/ui/path_text.rs';

/** A name or call that holds a file path: `path`, `row.path`, `saved_folder`, `download_dir`, `.display()`. A link's path (`link_path`, `url_path`) is not one. */
const PATH_LIKE =
  /\b(?!(?:link|url)_)(?:\w+_)?(?:path|folder|dir|directory)s?\b|\.display\(\)|\bto_string_lossy\(/;

/** Where the bracket closing at `close` opens. */
function openingBracket(code, close) {
  let depth = 0;
  for (let at = close; at >= 0; at -= 1) {
    if (')]}'.includes(code[at])) depth += 1;
    else if ('([{'.includes(code[at])) depth -= 1;
    if (depth === 0) return at;
  }
  return 0;
}

/** Where the method chain ending just before `end` starts: back over each `.name(args)` link to the call or name it starts from, `div()` or `row.path`. */
function chainStart(code, end) {
  let at = end;
  for (;;) {
    while (at > 0 && /\s/.test(code[at - 1])) at -= 1;
    if (code[at - 1] === ')') at = openingBracket(code, at - 1);
    while (at > 0 && /[\w:]/.test(code[at - 1])) at -= 1;
    let dot = at;
    while (dot > 0 && /\s/.test(code[dot - 1])) dot -= 1;
    if (code[dot - 1] !== '.') return at;
    at = dot - 1;
  }
}

/** Where the method chain going on at `start` ends: past each `.name(args)` link. */
function chainEnd(code, start) {
  const link = /^\s*\.\s*\w+(?:::<[^>]*>)?\s*\(?/;
  let at = start;
  for (let found = link.exec(code.slice(at)); found; found = link.exec(code.slice(at))) {
    at += found[0].length;
    if (found[0].endsWith('(')) at = closingBracket(code, at - 1);
  }
  return at;
}

/** `[start, end)` of the method chain that the `.truncate()` at `index` is a link of. */
function chainAround(code, index) {
  return [chainStart(code, index), chainEnd(code, index)];
}

/** The places in one Rust file where `.truncate()` cuts an element whose content is named as a path, as `{ line, code }`. */
export function pathsCutAtTheEnd(text) {
  const code = codeOnly(text);
  const lines = text.split('\n');
  const found = [];
  for (const match of code.matchAll(/\.truncate\(\)/g)) {
    const chain = code.slice(...chainAround(code, match.index));
    if (PATH_LIKE.test(chain) && !chain.includes('PathText')) {
      const line = code.slice(0, match.index).split('\n').length;
      found.push({ line, code: lines[line - 1].trim() });
    }
  }
  return found;
}

function pathText() {
  const hits = filesMatching(String.raw`\.truncate\(\)`, [SOURCES, `:!${PATH_TEXT}`]).flatMap(
    (file) =>
      pathsCutAtTheEnd(readFileSync(join(ROOT, file), 'utf8')).map(
        (hit) => `${file}:${String(hit.line)}: ${hit.code}`,
      ),
  );
  if (hits.length > 0) {
    throw new GateError(
      `Paths cut at their end, which hides the last folder or file name: show a path through ui::PathText, which cuts it in the middle (D239). Rename a value that only looks like a file path, such as a link's path:\n${hits.join('\n')}`,
    );
  }
}

export const PATH_TEXT_CHECKS = { 'path-text': pathText };
