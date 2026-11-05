// Refuses the product's name written in a Rust string: a rename or the development channel would leave it behind (D175).
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { filesMatching, GateError, ROOT } from './process.mjs';
import { closingBracket, codeOnly, splitTopLevel, stringLiterals } from './rust-attributes.mjs';

const PRODUCT_SOURCE = 'crates/product/src/lib.rs';
/** Crates for tests, and the crate that holds the name. */
const EXEMPT_CRATES = /^crates\/(?:e2e|product|test-support)\//;
const TEST_FILE = /(?:^|\/)tests\/|(?:^|\/)tests\.rs$/;
const TEST_ATTRIBUTE = /#\[(?:tokio::)?test\b|#\[cfg\(/g;
const FEATURE = /^feature\s*=/;
const MODULE_DECLARATION = /^(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*;/;
/** Files whose submodules sit beside them, not in a folder of their own name. */
const FOLDER_ROOT = /(?:^|\/)(?:lib|main|mod)\.rs$/;
/** Ids that stay: `assets/theme.json` names its themes with these. */
const THEME_IDS = 'crates/desktop/src/theme.rs';

/** Whether a `cfg` holds only in tests. `any(unix, test)` is in a person's build; `any(test, feature = "x")` counts as test code (D175). */
function testOnly(condition) {
  if (condition === 'test') return true;
  const arms = splitTopLevel(condition.slice(condition.indexOf('(') + 1, -1))
    .map((arm) => arm.trim())
    .filter(Boolean);
  if (condition.startsWith('all(')) return arms.some(testOnly);
  if (!condition.startsWith('any(')) return false;
  return arms.some(testOnly) && arms.every((arm) => testOnly(arm) || FEATURE.test(arm));
}

/** `[start, end)` of each attribute that keeps what follows out of a person's build. */
function testAttributes(code) {
  return [...code.matchAll(TEST_ATTRIBUTE)]
    .filter((match) => {
      if (!match[0].endsWith('(')) return true;
      const open = match.index + match[0].length - 1;
      return testOnly(code.slice(open + 1, closingBracket(code, open) - 1).trim());
    })
    .map((match) => [match.index, closingBracket(code, match.index + 1)]);
}

/** Where the item from `start` ends: past its body, or at its semicolon outside brackets. */
function itemEnd(code, start) {
  let depth = 0;
  for (let i = start; i < code.length; i += 1) {
    if ('(['.includes(code[i])) depth += 1;
    else if (')]'.includes(code[i])) depth -= 1;
    else if (depth === 0 && code[i] === ';') return i + 1;
    else if (depth === 0 && code[i] === '{') return closingBracket(code, i);
  }
  return code.length;
}

/** The lines of `text` with `name` as a word in a string a person's build holds. */
export function namesInStrings(text, name) {
  const code = codeOnly(text);
  const tests = testAttributes(code).map(([start, end]) => [start, itemEnd(code, end)]);
  const word = new RegExp(String.raw`\b${name}\b`);
  return stringLiterals(text)
    .filter(([start]) => tests.every(([from, to]) => start < from || start >= to))
    .filter(([start, end]) => word.test(text.slice(start, end)))
    .map(([start]) => text.slice(0, start).split('\n').length);
}

/** The module paths, without `.rs`, that `file` declares for tests only (`#[cfg(test)] mod fake;`). */
export function testModuleFolders(file, text) {
  const code = codeOnly(text);
  const folder = FOLDER_ROOT.test(file)
    ? file.slice(0, file.lastIndexOf('/'))
    : file.replace(/\.rs$/, '');
  return testAttributes(code).flatMap(([, end]) => {
    const declared = MODULE_DECLARATION.exec(code.slice(end).trimStart());
    return declared ? [`${folder}/${declared[1]}`] : [];
  });
}

export function isTestModuleFile(file, folders) {
  return folders.some((folder) => file === `${folder}.rs` || file.startsWith(`${folder}/`));
}

function productNameInStrings() {
  const found = /\bPRODUCT_NAME: &str = "(\w+)"/.exec(
    readFileSync(join(ROOT, PRODUCT_SOURCE), 'utf8'),
  );
  if (!found) throw new GateError(`${PRODUCT_SOURCE} no longer names PRODUCT_NAME.`);
  const name = found[1];
  const folders = filesMatching(String.raw`\bmod\s+\w+\s*;`, ['crates/*.rs']).flatMap((file) =>
    testModuleFolders(file, readFileSync(join(ROOT, file), 'utf8')),
  );
  const hits = filesMatching(String.raw`\b${name}\b`, ['crates/*.rs'])
    .filter((file) => !EXEMPT_CRATES.test(file) && !TEST_FILE.test(file) && file !== THEME_IDS)
    .filter((file) => !isTestModuleFile(file, folders))
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
