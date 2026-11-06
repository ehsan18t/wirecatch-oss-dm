// `path-text`: a path the desktop app shows is cut in its middle by `ui::PathText`, never at its end by `.truncate()`, which hides the last folder or file name (D239). Paths are told apart by name, since a text check cannot see types.
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { filesMatching, GateError, ROOT } from './process.mjs';
import { codeOnly } from './rust-attributes.mjs';

const SOURCES = 'crates/desktop/src/*.rs';
const PATH_TEXT = 'crates/desktop/src/ui/path_text.rs';

/** A name or call that holds a file path: `path`, `row.path`, `saved_folder`, `download_dir`, `.display()`. A link's path (`link_path`, `url_path`) is not one. */
const PATH_LIKE =
  /\b(?!(?:link|url)_)(?:\w+_)?(?:path|folder|dir|directory)s?\b|\.display\(\)|\bto_string_lossy\(/;

/** Where the builder chain at `index` ends going `step` (1 or -1): at the bracket that holds it, or at a `,` or `;` beside it. */
function chainEdge(code, index, step) {
  const [opening, closing] = step > 0 ? ['([{', ')]}'] : [')]}', '([{'];
  let at = index;
  let depth = 0;
  for (; at >= 0 && at < code.length; at += step) {
    const c = code[at];
    if (opening.includes(c)) depth += 1;
    else if (closing.includes(c) && depth > 0) depth -= 1;
    else if (depth === 0 && (closing.includes(c) || ',;'.includes(c))) break;
  }
  return at;
}

/** `[start, end)` of the builder chain around `index`. */
function chainAround(code, index) {
  return [chainEdge(code, index - 1, -1) + 1, chainEdge(code, index, 1)];
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
