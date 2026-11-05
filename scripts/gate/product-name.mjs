// Refuses the product's name written in a Rust string: a rename or the development channel would leave it behind (D175).
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { filesMatching, GateError, ROOT } from './process.mjs';
import { codeOnly, stringLiterals } from './rust-attributes.mjs';
import { declaredTestModules, isTestFile, testItems } from './test-code.mjs';

const PRODUCT_SOURCE = 'crates/product/src/lib.rs';
/** Crates for tests, and the crate that holds the name. */
const EXEMPT_CRATES = /^crates\/(?:e2e|product|test-support)\//;
/** Ids that stay: `assets/theme.json` names its themes with these. */
const THEME_IDS = 'crates/desktop/src/theme.rs';

/** The lines of `text` with `name` as a word in a string a person's build holds. */
export function namesInStrings(text, name) {
  const tests = testItems(codeOnly(text));
  const word = new RegExp(String.raw`\b${name}\b`);
  return stringLiterals(text)
    .filter(([start]) => tests.every(([from, to]) => start < from || start >= to))
    .filter(([start, end]) => word.test(text.slice(start, end)))
    .map(([start]) => text.slice(0, start).split('\n').length);
}

function productNameInStrings() {
  const found = /\bPRODUCT_NAME: &str = "(\w+)"/.exec(
    readFileSync(join(ROOT, PRODUCT_SOURCE), 'utf8'),
  );
  if (!found) throw new GateError(`${PRODUCT_SOURCE} no longer names PRODUCT_NAME.`);
  const name = found[1];
  const testModules = declaredTestModules();
  const hits = filesMatching(String.raw`\b${name}\b`, ['crates/*.rs'])
    .filter((file) => !EXEMPT_CRATES.test(file) && file !== THEME_IDS)
    .filter((file) => !isTestFile(file, testModules))
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
