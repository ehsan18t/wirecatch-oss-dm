// Commit messages follow docs/ENGINEERING.md, section 9: the `commits` check over a range, and the commit-msg hook for one message.
import { existsSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { capture, GateError, ROOT } from './process.mjs';

/** Conventional Commit types and package scopes allowed in commit subjects (docs/ENGINEERING.md, section 9). */
const COMMIT_TYPES = [
  'feat',
  'fix',
  'docs',
  'test',
  'refactor',
  'perf',
  'build',
  'ci',
  'chore',
  'style',
  'revert',
];
const COMMIT_SCOPES = [
  'engine',
  'protocol',
  'core',
  'ipc',
  'bridge',
  'streams',
  'queues',
  'server',
  'sites',
  'files',
  'addons',
  'records',
  'store',
  'cli',
  'e2e',
  'test-support',
  'desktop',
  'extension',
  'units',
  'ui',
  'eslint-config',
];
const MAX_SUBJECT_LENGTH = 72;
/** Lines that credit a tool or an AI as an author. Commits are authored by the person who owns the repository. */
const AUTHOR_CREDITS = [
  /^co-authored-by:/im,
  /noreply@anthropic\.com/i,
  /^\W*generated with \[[^\]]+\]\(https?:/im,
];
/** Subjects git writes for `--fixup`, `--squash` and `--fixup=amend:`; they disappear in an autosquash rebase. */
const AUTOSQUASH = /^(?:fixup|squash|amend)! /;
/** Where `git commit -v` starts the diff it appends to the message. */
const SCISSORS = /^# -+ >8 -+$/m;
/** Bots whose commit bodies quote upstream release notes we do not control; only their subjects are checked. */
const BOT_AUTHORS = /\[bot\]@users\.noreply\.github\.com$/;
/** The character the rules ban, written by its code so this file does not contain it. */
const EM_DASH = String.fromCharCode(0x2014);
const SUBJECT = new RegExp(
  String.raw`^(?:${COMMIT_TYPES.join('|')})(?:\((?:${COMMIT_SCOPES.join('|')})\))?!?: (?<words>.+)$`,
);

/** What a commit message says, without git's comment lines or the diff `git commit -v` appends. */
function messageText(raw) {
  const normalized = raw.replaceAll(/\r\n?/g, '\n');
  const scissors = SCISSORS.exec(normalized);
  const kept = scissors ? normalized.slice(0, scissors.index) : normalized;
  return kept
    .split('\n')
    .filter((line) => !line.startsWith('#'))
    .join('\n')
    .trim();
}

/** What is wrong with a subject line, or an empty list. */
function subjectProblems(subject) {
  const problems = [];
  const match = SUBJECT.exec(subject);
  if (match) {
    const words = match.groups.words;
    if (/^[A-Z]/.test(words)) problems.push('the subject must start in lower case');
    if (words.endsWith('.')) problems.push('the subject must not end with a period');
  } else {
    const revert = subject.startsWith('Revert "')
      ? ' For a revert, write `revert: <the original subject>`.'
      : '';
    problems.push(
      `the subject must look like \`type(scope): subject\`, with a type from ${COMMIT_TYPES.join(', ')} and a scope from ${COMMIT_SCOPES.join(', ')} (or no scope for a repository-wide change).${revert}`,
    );
  }
  if (subject.length > MAX_SUBJECT_LENGTH) {
    problems.push(
      `the subject is ${String(subject.length)} characters; the limit is ${String(MAX_SUBJECT_LENGTH)}`,
    );
  }
  return problems;
}

/** Returns what is wrong with one commit message, or an empty list. With `subjectOnly`, the body is not checked. */
function commitProblems(raw, { subjectOnly = false } = {}) {
  const text = messageText(raw);
  const [subject = '', ...rest] = text.split('\n');
  if (AUTOSQUASH.test(subject)) return [];
  const problems = subjectProblems(subject);
  const checked = subjectOnly ? subject : text;
  if (checked.includes(EM_DASH)) problems.push('the message contains an em dash');
  if (!subjectOnly && AUTHOR_CREDITS.some((credit) => credit.test(rest.join('\n')))) {
    problems.push('the message credits a tool or AI as an author; remove that line');
  }
  return problems;
}

/** Merge commits are not checked: git writes their messages, and the `chore: merge` form for merges into main is a review rule. */
function commitRange() {
  const range = process.env.GATE_COMMIT_RANGE ?? 'origin/main..HEAD';
  const hint =
    'Fetch the main branch (`git fetch origin main`) or set GATE_COMMIT_RANGE to the commits to check.';
  const shas = capture('git', ['rev-list', '--no-merges', range], { hint })
    .out.split('\n')
    .filter(Boolean);
  const failures = [];
  for (const sha of shas) {
    const [author, ...message] = capture('git', ['log', '-1', '--format=%ae%n%B', sha]).out.split(
      '\n',
    );
    const problems = commitProblems(message.join('\n'), { subjectOnly: BOT_AUTHORS.test(author) });
    if (problems.length > 0)
      failures.push(`${sha.slice(0, 9)} ${message[0]}\n  - ${problems.join('\n  - ')}`);
  }
  if (failures.length > 0)
    throw new GateError(
      `Commit messages that break docs/ENGINEERING.md, section 9:\n${failures.join('\n')}`,
    );
  console.log(`${String(shas.length)} commit message(s) in ${range} follow the rules.`);
}

/** The commit-msg hook: checks the message in `file`. */
export function commitMessageFile(file) {
  if (!file) throw new GateError('commit-msg needs the path of the message file.');
  // A merge's message is written by git (see commitRange).
  const mergeHead = capture('git', ['rev-parse', '--git-path', 'MERGE_HEAD']).out.trim();
  if (existsSync(resolve(ROOT, mergeHead))) return;
  const problems = commitProblems(readFileSync(file, 'utf8'));
  if (problems.length > 0)
    throw new GateError(
      `This commit message breaks docs/ENGINEERING.md, section 9:\n  - ${problems.join('\n  - ')}`,
    );
}

export const COMMIT_CHECKS = { commits: commitRange };
