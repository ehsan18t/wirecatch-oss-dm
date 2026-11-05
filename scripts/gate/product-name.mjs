// Refuses the product's name written in a Rust string: a rename or the development channel would leave it behind (D175).
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { filesMatching, GateError, ROOT } from './process.mjs';
import { closingBracket, codeOnly, stringLiterals } from './rust-attributes.mjs';

const PRODUCT_SOURCE = 'crates/product/src/lib.rs';
/** Crates for tests, and the crate that holds the name. */
const EXEMPT_CRATES = /^crates\/(?:e2e|product|test-support)\//;
const TEST_FILE = /(?:^|\/)tests\/|(?:^|\/)tests\.rs$/;
const TEST_MODULE =
  /#\[cfg\((?:all\()?test\b[^\]]*\]\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?mod\s+\w+\s*\{/g;
/** Ids that stay: `assets/theme.json` names its themes with these. */
const THEME_IDS = 'crates/desktop/src/theme.rs';

/** The lines of `text` with `name` as a word in a string outside test modules. */
export function namesInStrings(text, name) {
  const code = codeOnly(text);
  const tests = [...code.matchAll(TEST_MODULE)].map((match) => {
    const open = match.index + match[0].length - 1;
    return [match.index, closingBracket(code, open)];
  });
  const word = new RegExp(String.raw`\b${name}\b`);
  return stringLiterals(text)
    .filter(([start]) => tests.every(([from, to]) => !(start >= from) || !(start < to)))
    .filter(([start, end]) => word.test(text.slice(start, end)))
    .map(([start]) => text.slice(0, start).split('\n').length);
}

function productNameInStrings() {
  const found = /\bPRODUCT_NAME: &str = "(\w+)"/.exec(
    readFileSync(join(ROOT, PRODUCT_SOURCE), 'utf8'),
  );
  if (!found) throw new GateError(`${PRODUCT_SOURCE} no longer names PRODUCT_NAME.`);
  const name = found[1];
  const hits = filesMatching(String.raw`\b${name}\b`, ['crates/*.rs'])
    .filter((file) => !EXEMPT_CRATES.test(file) && !TEST_FILE.test(file) && file !== THEME_IDS)
    .flatMap((file) =>
      namesInStrings(readFileSync(join(ROOT, file), 'utf8'), name).map(
        (line) => `${file}:${String(line)}`,
      ),
    );
  if (hits.length > 0) {
    throw new GateError(
      `The product's name written in a string:\n${hits.join('\n')}\nRead PRODUCT_NAME from wirecatch-product below the shell, and the shell's CHANNEL.name() in and above it (D175).`,
    );
  }
}

export const PRODUCT_NAME_CHECKS = { 'product-name': productNameInStrings };
