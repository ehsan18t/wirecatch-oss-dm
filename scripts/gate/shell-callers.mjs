// Public items of the shell library with no caller. The compiler reports none: to it, a library's public items are all in use.
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { capture, GateError, ROOT } from './process.mjs';

const SHELL_SOURCE = 'crates/shell/src/';
/** Crates that exist for the tests: what only they call has no caller in the product. */
const TEST_CRATES = ['crates/e2e/', 'crates/test-support/'];

const PUBLIC_ITEM =
  /^\s*pub\s+(?:(?:async|const|unsafe)\s+)*(?:fn|struct|enum|trait|type|const|static|mod)\s+(\w+)/gm;
const LINE_COMMENT = /\/\/.*$/gm;
const TEST_MODULE = /^(\s*)#\[cfg\(test\)\][\t\v\f\r \xa0\u1680\u2000-\u200a\u2028\u2029\u202f\u205f\u3000\ufeff]*\n\s*(?:pub\s+)?mod\s+\w+\s*\{[\s\S]*?^\1\}/gm;
const WORD = /\w+/g;

/** The code of a Rust file that a person's build runs: no comments, no test modules. */
export function productCode(source) {
  return source.replaceAll(LINE_COMMENT, '').replaceAll(TEST_MODULE, '');
}

export function publicItems(code) {
  return [...code.matchAll(PUBLIC_ITEM)].map((match) => match[1]);
}

/**
 * The public items of the shell files that nothing else names, as `file: name`, from a map of file to product code. Matches words, not paths, so a name another item also has counts as used: the check can miss a fault, never invent one.
 */
export function itemsWithoutCallers(codeByFile) {
  const uses = new Map();
  for (const code of codeByFile.values()) {
    for (const [word] of code.matchAll(WORD)) uses.set(word, (uses.get(word) ?? 0) + 1);
  }
  const defined = new Map();
  const found = [];
  for (const [file, code] of codeByFile) {
    if (!file.startsWith(SHELL_SOURCE)) continue;
    for (const name of publicItems(code)) {
      defined.set(name, (defined.get(name) ?? 0) + 1);
      found.push([file, name]);
    }
  }
  return found
    .filter(([, name]) => uses.get(name) === defined.get(name))
    .map(([file, name]) => `${file}: ${name}`);
}

function productFiles() {
  return capture('git', ['ls-files', '--', 'crates/*/src/*.rs'])
    .out.split('\n')
    .filter(Boolean)
    .filter((file) => TEST_CRATES.every((crate) => !file.startsWith(crate)))
    .filter((file) => !/(?:^|\/)tests?\.rs$/.test(file));
}

function shellCallers() {
  const codeByFile = new Map(
    productFiles().map((file) => [file, productCode(readFileSync(join(ROOT, file), 'utf8'))]),
  );
  const unused = itemsWithoutCallers(codeByFile);
  if (unused.length > 0) {
    throw new GateError(
      `Public items of crates/shell with no caller outside tests:\n${unused.join('\n')}\nRemove them, or make them private if only their own file uses them.`,
    );
  }
}

export const SHELL_CALLER_CHECKS = { 'shell-callers': shellCallers };
