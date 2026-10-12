// Reads the comments that switch a lint off in TypeScript, JavaScript and Svelte: ESLint disable comments and svelte-ignore comments.
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { filesMatching, ROOT } from './process.mjs';

/** Files that may hold disable comments: everything ESLint and Svelte read, except the gate's own modules, whose patterns name them. */
const COMMENT_SCOPE = [
  '*.ts',
  '*.js',
  '*.mjs',
  '*.cjs',
  '*.svelte',
  ':!scripts/gate.mjs',
  ':!scripts/gate/**',
];
/** An ESLint disable comment in any of its forms, with the rules and reason that follow it. */
const ESLINT_DISABLE =
  /(?:\/\/|\/\*|<!--)\s*eslint-disable(?:-next-line|-line)?(?=\s|\*\/|-->|$)(.*?)(?:\*\/|-->|$)/gm;
/** A svelte-ignore comment, in markup or in a script, with the codes and reason that follow it. */
const SVELTE_IGNORE = /(?:<!--|\/\/|\/\*)\s*svelte-ignore\s(.*?)(?:-->|\*\/|$)/gm;
/** Svelte reads the codes, separated by commas, from the start of a svelte-ignore comment and treats what follows as prose. */
const SVELTE_IGNORE_CODES = /^\s*[\w$-]+(?:\s*,\s*[\w$-]+)*/;
/** Where a comment's reason starts: ESLint's `--` separator, which svelte-ignore comments use too. */
const REASON_SEPARATOR = /(?:^|\s)--+\s+\S/;

/** The comments of one file. */
function commentsIn(file) {
  const text = readFileSync(join(ROOT, file), 'utf8');
  const lineOf = (index) => text.slice(0, index).split('\n').length;
  const found = [];
  for (const match of text.matchAll(ESLINT_DISABLE)) {
    const [rules] = match[1].split(/\s--+\s/);
    const lints = rules
      .split(',')
      .map((rule) => rule.trim())
      .filter(Boolean);
    found.push({
      file,
      line: lineOf(match.index),
      kind: 'eslint-disable',
      lints: lints.length > 0 ? lints.sort() : ['(every rule)'],
      reason: REASON_SEPARATOR.test(match[1]),
    });
  }
  for (const match of text.matchAll(SVELTE_IGNORE)) {
    const codes = SVELTE_IGNORE_CODES.exec(match[1])?.[0] ?? '';
    const lints = codes
      .split(',')
      .map((code) => code.trim())
      .filter(Boolean);
    found.push({
      file,
      line: lineOf(match.index),
      kind: 'svelte-ignore',
      lints: lints.sort(),
      reason: REASON_SEPARATOR.test(match[1].slice(codes.length).trim()),
    });
  }
  return found;
}

let cached = null;

/** Every ESLint disable comment and svelte-ignore comment: the file and line it is on, what it switches off, and whether it gives a reason. Read once per gate run. */
export function disableComments() {
  cached ??= filesMatching('eslint-disable|svelte-ignore', COMMENT_SCOPE).flatMap(commentsIn);
  return cached;
}
