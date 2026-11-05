// What counts as test code in the Rust sources, for the checks that treat it differently: whole files, and single items only tests build (D175).
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { filesMatching, ROOT } from './process.mjs';
import { closingBracket, codeOnly, splitTopLevel } from './rust-attributes.mjs';

/** A file that holds nothing but tests: an integration test, or a `tests.rs` module. */
const TEST_FILE = /(?:^|\/)tests\/|(?:^|\/)tests\.rs$/;
const TEST_ATTRIBUTE = /#\[(?:tokio::)?test\b|#\[cfg\(/g;
const FEATURE = /^feature\s*=/;
const MODULE_DECLARATION = /^(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*;/;
/** Files whose submodules sit beside them, not in a folder of their own name. */
const FOLDER_ROOT = /(?:^|\/)(?:lib|main|mod)\.rs$/;

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

/** Where the item from `start` ends: past its body, at its semicolon outside brackets, or at the `}` around a field. */
function itemEnd(code, start) {
  let depth = 0;
  for (let i = start; i < code.length; i += 1) {
    if ('(['.includes(code[i])) depth += 1;
    else if (')]'.includes(code[i])) depth -= 1;
    else if (depth === 0 && code[i] === ';') return i + 1;
    else if (depth === 0 && code[i] === '{') return closingBracket(code, i);
    else if (depth === 0 && code[i] === '}') return i;
  }
  return code.length;
}

/** `[start, end)` of each item in `code` only tests build, its attribute included. `code` is the file after `codeOnly`. */
export function testItems(code) {
  return testAttributes(code).map(([start, end]) => [start, itemEnd(code, end)]);
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

/** Every test-only module path the Rust sources declare, for `isTestFile`. */
export function declaredTestModules() {
  return filesMatching(String.raw`\bmod\s+\w+\s*;`, ['crates/*.rs']).flatMap((file) =>
    testModuleFolders(file, readFileSync(join(ROOT, file), 'utf8')),
  );
}

/** Whether all of `file` is test code, given the module paths `declaredTestModules` found. */
export function isTestFile(file, testModules) {
  return (
    TEST_FILE.test(file) ||
    testModules.some((module) => file === `${module}.rs` || file.startsWith(`${module}/`))
  );
}
