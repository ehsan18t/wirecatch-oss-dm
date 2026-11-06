// `discarded`: refuses a Rust statement `_ = expr;`, which drops a result without a word, and which clippy's `let_underscore_must_use` does not see since it is no `let` (ENGINEERING section 4). A `tokio::select!` arm (`_ = tick.tick() => ..`) is not one.
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { filesMatching, GateError, ROOT } from './process.mjs';
import { codeOnly } from './rust-attributes.mjs';

/** What ends the expression starting at `start`: `;` or a closing bracket for a statement, `=>` or `,` for a `select!` arm. */
function ending(code, start) {
  let depth = 0;
  for (let at = start; at < code.length; at += 1) {
    const c = code[at];
    if ('([{'.includes(c)) depth += 1;
    else if (')]}'.includes(c)) {
      if (depth === 0) return c;
      depth -= 1;
    } else if (depth === 0 && (c === ';' || c === ',' || code.startsWith('=>', at))) {
      return code.startsWith('=>', at) ? '=>' : c;
    }
  }
  return ';';
}

/** The lines of one Rust file that drop a result with `_ = expr;`, as `{ line, code }`. */
export function discardedResults(text) {
  const code = codeOnly(text);
  const lines = text.split('\n');
  return [...code.matchAll(/^[ \t]*_ =(?![=>])\s*/gm)]
    .filter((match) => !['=>', ','].includes(ending(code, match.index + match[0].length)))
    .map((match) => {
      const line = code.slice(0, match.index).split('\n').length;
      return { line, code: lines[line - 1].trim() };
    });
}

function discarded() {
  const hits = filesMatching(String.raw`^\s*_ =`, ['*.rs']).flatMap((file) =>
    discardedResults(readFileSync(join(ROOT, file), 'utf8')).map(
      (hit) => `${file}:${String(hit.line)}: ${hit.code}`,
    ),
  );
  if (hits.length > 0) {
    throw new GateError(
      `Results dropped with \`_ = expr;\` (docs/ENGINEERING.md, section 4): handle the failure, propagate it, log it, or pass the result to a small function whose name says why it cannot matter (\`browser_may_be_gone\`):\n${hits.join('\n')}`,
    );
  }
}

export const DISCARDED_CHECKS = { discarded };
