// Reads the comments that switch a lint off or configure the linter in place, in TypeScript, JavaScript and Svelte: ESLint disable comments, ESLint's rule, globals and environment comments, and svelte-ignore comments. The gate's own modules are read too, so their patterns and messages never write such a comment whole.
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { filesMatching, ROOT } from './process.mjs';

/** Files that may hold such comments: everything ESLint and Svelte read. */
const COMMENT_SCOPE = [
  '*.ts',
  '*.mts',
  '*.cts',
  '*.tsx',
  '*.js',
  '*.mjs',
  '*.cjs',
  '*.jsx',
  '*.svelte',
];
/** What a file must contain to be read at all. */
const ANY_DIRECTIVE = 'eslint|svelte-ignore|/\\*\\s*(globals?|exported)\\s';
/** The start of a comment, in a script (`//`, `/*`) or in Svelte markup (`<!--`). */
const OPEN = String.raw`(\/\/|\/\*|<!--)\s*`;
/** The end of a comment's text: its closing marker, or the end of the line. */
const CLOSE = String.raw`(.*?)(?:\*\/|-->|$)`;
/** An ESLint disable comment in any of its forms, with the rules and reason that follow it. */
const ESLINT_DISABLE = new RegExp(
  `${OPEN}eslint-disable(-next-line|-line)?(?=\\s|\\*\\/|-->|$)${CLOSE}`,
  'gm',
);
/** ESLint configuration written in a comment: rule settings, globals and environments. ESLint reads these from block comments only. */
const INLINE_CONFIG = new RegExp(`${OPEN}(eslint|globals?|eslint-env|exported)\\s${CLOSE}`, 'gm');
/** A svelte-ignore comment, in markup or in a script, with the codes and reason that follow it. */
const SVELTE_IGNORE = new RegExp(`${OPEN}svelte-ignore\\s${CLOSE}`, 'gm');
/** Svelte reads the codes, separated by commas, from the start of a svelte-ignore comment and treats what follows as prose. */
const SVELTE_IGNORE_CODES = /^\s*[\w$-]+(?:\s*,\s*[\w$-]+)*/;
/** Where a comment's reason starts: ESLint's `--` separator, which svelte-ignore comments use too. */
const REASON_SEPARATOR = /(?:^|\s)--+\s+\S/;
const REASON_SPLIT = /\s--+\s/;

/** The names a comment's list gives, before its reason: `a, b -- why` gives `a` and `b`. */
function listedNames(text) {
  const [names] = text.split(REASON_SPLIT);
  return names
    .split(',')
    .map((name) => name.trim())
    .filter(Boolean)
    .sort();
}

/** The rules an inline `eslint` comment configures: the names before each `:`, outside the brackets of their settings. */
function configuredRules(text) {
  let settings = text.split(REASON_SPLIT)[0];
  while (/[[{][^[\]{}]*[\]}]/.test(settings))
    settings = settings.replace(/[[{][^[\]{}]*[\]}]/g, '');
  return [...settings.matchAll(/([@\w][\w@/-]*)\s*:/g)].map((m) => m[1]).sort();
}

function eslintDisables(text, site) {
  return [...text.matchAll(ESLINT_DISABLE)].map((match) => {
    const lints = listedNames(match[3]);
    return {
      ...site(match),
      kind: `eslint-disable${match[2] ?? ''}`,
      markup: match[1] === '<!--',
      lints: lints.length > 0 ? lints : ['(every rule)'],
      reason: REASON_SEPARATOR.test(match[3]),
    };
  });
}

function inlineConfigs(text, site) {
  return [...text.matchAll(INLINE_CONFIG)]
    .filter((match) => match[1] !== '//')
    .map((match) => {
      const kind = match[2] === 'eslint' ? 'eslint inline config' : match[2];
      const lints = match[2] === 'eslint' ? configuredRules(match[3]) : listedNames(match[3]);
      return {
        ...site(match),
        kind,
        markup: match[1] === '<!--',
        lints,
        reason: REASON_SEPARATOR.test(match[3]),
      };
    });
}

function svelteIgnores(text, site) {
  return [...text.matchAll(SVELTE_IGNORE)].map((match) => {
    const codes = SVELTE_IGNORE_CODES.exec(match[2])?.[0] ?? '';
    const reason = REASON_SEPARATOR.test(match[2].slice(codes.length).trim());
    return {
      ...site(match),
      kind: 'svelte-ignore',
      markup: match[1] === '<!--',
      lints: listedNames(codes),
      reason,
    };
  });
}

/** The comments of one file. */
function commentsIn(file) {
  const text = readFileSync(join(ROOT, file), 'utf8');
  const site = (match) => ({ file, line: text.slice(0, match.index).split('\n').length });
  return [
    ...eslintDisables(text, site),
    ...inlineConfigs(text, site),
    ...svelteIgnores(text, site),
  ];
}

let cached = null;

/** Every such comment: the file and line it is on, its kind, whether it is in Svelte markup, what it switches off or configures, and whether it gives a reason. Read once per gate run. */
export function disableComments() {
  cached ??= filesMatching(ANY_DIRECTIVE, COMMENT_SCOPE).flatMap(commentsIn);
  return cached;
}
