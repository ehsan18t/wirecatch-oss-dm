// `discarded`: refuses `_ = expr` in Rust, as a statement, a match arm or a closure's body, which drops a result without a word, and which clippy's `let_underscore_must_use` does not see since it is no `let` (ENGINEERING section 4). A `tokio::select!` pattern (`_ = tick.tick() => ..`) is not one.
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { filesMatching, GateError, ROOT } from './process.mjs';
import { codeOnly } from './rust-attributes.mjs';

/** The end that a top-level `at` makes, if any: a `,` before a `select!` arm's `if` precondition is none. */
function endAt(code, at) {
  if (code.startsWith('=>', at)) return '=>';
  if (code[at] === ';') return ';';
  if (code[at] === ',' && !/^,\s*if\b/.test(code.slice(at, at + 64))) return ',';
  return null;
}

/** What ends the expression starting at `start`: `=>` only for a `select!` pattern. */
function ending(code, start) {
  let depth = 0;
  for (let at = start; at < code.length; at += 1) {
    const c = code[at];
    if ('([{'.includes(c)) depth += 1;
    else if (')]}'.includes(c)) {
      if (depth === 0) return c;
      depth -= 1;
    } else if (depth === 0 && endAt(code, at) !== null) {
      return endAt(code, at);
    }
  }
  return ';';
}

/** The lines of one Rust file that drop a result with `_ = expr`, as `{ line, code }`. */
export function discardedResults(text) {
  const code = codeOnly(text);
  const lines = text.split('\n');
  return [...code.matchAll(/(?<=(?:^|[{};|>(,])\s*)_ =(?![=>])\s*/g)]
    .filter((match) => ending(code, match.index + match[0].length) !== '=>')
    .map((match) => {
      const line = code.slice(0, match.index).split('\n').length;
      return { line, code: lines[line - 1].trim() };
    });
}

function discarded() {
  const hits = filesMatching(String.raw`\b_ =`, ['*.rs']).flatMap((file) =>
    discardedResults(readFileSync(join(ROOT, file), 'utf8')).map(
      (hit) => `${file}:${String(hit.line)}: ${hit.code}`,
    ),
  );
  if (hits.length > 0) {
    throw new GateError(
      `Results dropped with \`_ = expr\` (docs/ENGINEERING.md, section 4): handle the failure, propagate it, log it, or pass the result to a small function whose name says why it cannot matter (\`browser_may_be_gone\`):\n${hits.join('\n')}`,
    );
  }
}

export const DISCARDED_CHECKS = { discarded };
