#!/usr/bin/env node
// The quality gate: every check a change must pass, in one place. CI, CLAUDE.md, CONTRIBUTING.md and scripts/linux-test.sh run this script instead of repeating its commands, so they cannot drift apart.
//
//   node scripts/gate.mjs                  every check CI runs (see DEFAULT)
//   node scripts/gate.mjs rust js          named checks or groups (see GROUPS)
//   node scripts/gate.mjs commit-msg FILE  check one commit message (the commit-msg hook)
//   node scripts/gate.mjs --install-tools  install the cargo tools some checks need, at the versions in TOOLS
//   node scripts/gate.mjs --list           list checks and groups
//
// Options, before or after the names:
//   --no-desktop    leave out the desktop crate, for machines without its system libraries (it says so when it does)
//   --no-fail-fast  run every Rust test binary even after one fails
//
// The commit range for `commits` is GATE_COMMIT_RANGE when set, otherwise `origin/main..HEAD`. Written for Node 18 and later, so it also runs where only an older Node is installed (scripts/linux-test.sh).

import { execFileSync, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');

/** Features every workspace-wide cargo command needs. `test-overrides` exists for the bridge's end-to-end tests only and is never enabled in a build. */
const TEST_FEATURES = ['--features', 'wirecatch-bridge/test-overrides'];

/**
 * Crates that do not yet opt into `[workspace.lints]`. A crate leaves the list in the change that makes it lint-clean (docs/AUDIT.md, W14), and the check fails if a listed crate has opted in or no longer exists. That the list never grows is a review rule: a new crate opts in from the start.
 */
const LINT_PENDING = new Set([
  'wirecatch-bridge',
  'wirecatch-core',
  'wirecatch-desktop',
  'wirecatch-e2e',
  'wirecatch-streams',
]);

/** Cargo tools some checks run, with the versions CI installs (`--install-tools`). */
const TOOLS = {
  'cargo-deny': '0.19.0',
  'cargo-machete': '0.9.2',
};

/** Conventional Commit types and package scopes allowed in commit subjects (docs/ENGINEERING.md, section 9). */
const COMMIT_TYPES = ['feat', 'fix', 'docs', 'test', 'refactor', 'perf', 'build', 'ci', 'chore', 'style', 'revert'];
const COMMIT_SCOPES = ['engine', 'protocol', 'core', 'ipc', 'bridge', 'streams', 'cli', 'e2e', 'desktop', 'extension'];
const MAX_SUBJECT_LENGTH = 72;
/** Lines that credit a tool or an AI as an author. Commits are authored by the person who owns the repository. */
const AUTHOR_CREDITS = [/^co-authored-by:/im, /noreply@anthropic\.com/i, /^\W*generated with \[[^\]]+\]\(https?:/im];
/** Subjects git writes for `--fixup`, `--squash` and `--fixup=amend:`; they disappear in an autosquash rebase. */
const AUTOSQUASH = /^(fixup|squash|amend)! /;
/** Where `git commit -v` starts the diff it appends to the message. */
const SCISSORS = /^# -+ >8 -+$/m;
/** Bots whose commit bodies quote upstream release notes we do not control; only their subjects are checked. */
const BOT_AUTHORS = /\[bot\]@users\.noreply\.github\.com$/;

/** Ways an em dash gets into the repository. Escapes only count in TypeScript, JavaScript and Svelte outside tests, where interface text lives: tests use them to assert there is none, and Rust tests do the same inside `#[cfg(test)]` modules that a path cannot tell apart. */
// The entity is split so this line does not match itself.
const EM_DASH_LITERAL = '\\x{2014}|&' + 'mdash;';
const EM_DASH_ESCAPED = '\\\\u2014|\\\\u\\{2014\\}';
const TEXT_SCOPE = ['.', ':!Cargo.lock', ':!pnpm-lock.yaml'];
const ESCAPE_SCOPE = ['*.ts', '*.js', '*.mjs', '*.svelte', ':!scripts/gate.mjs', ':!**/tests/**', ':!**/*test*', ':!**/testing/**'];

/** Rust and JS packages that must stay on the same major.minor (Tauri warns at build time when they differ). */
const TAURI_PAIRS = [
  ['tauri', '@tauri-apps/api'],
  ['tauri-plugin-dialog', '@tauri-apps/plugin-dialog'],
];

const DESKTOP_CRATE = 'wirecatch-desktop';
const DESKTOP_APP = 'apps/desktop';
const TAURI_CONFIG = 'apps/desktop/src-tauri/tauri.conf.json';
const ENGINE_CRATE = 'wirecatch-engine';

/** Command-line options. */
const options = { desktop: true, failFast: true };

// ---------------------------------------------------------------------------------------------
// Running commands

class GateError extends Error {}

/** Upper bound on output read back from a command (cargo metadata of the whole workspace is a few MiB). */
const MAX_CAPTURED_BYTES = 64 * 1024 * 1024;

/**
 * How to start a command. pnpm is a `.cmd` script on Windows and only starts through a shell, so it is passed as one command line built from this script's own constant arguments. cargo, git and rustup are real executables and start without a shell, so `%` and quotes in their arguments reach them unchanged.
 */
function invocation(command, args) {
  if (process.platform === 'win32' && command === 'pnpm') {
    return { file: [command, ...args].join(' '), args: [], shell: true };
  }
  return { file: command, args, shell: false };
}

function describe(command, args) {
  return `\`${[command, ...args].join(' ')}\``;
}

function run(command, args, { env } = {}) {
  console.log(`\n$ ${[command, ...args].join(' ')}`);
  const how = invocation(command, args);
  const result = spawnSync(how.file, how.args, {
    cwd: ROOT,
    stdio: 'inherit',
    shell: how.shell,
    env: { ...process.env, ...env },
  });
  if (result.error) {
    throw new GateError(`Could not start ${describe(command, args)}: ${result.error.message}. Check that ${command} is installed and on your PATH.`);
  }
  if (result.status !== 0) {
    throw new GateError(`${describe(command, args)} failed with status ${String(result.status)}.`);
  }
}

/** Runs a command and returns what it printed. `allowed` lists exit statuses that are answers rather than failures (git grep exits 1 when nothing matches). */
function capture(command, args, { allowed = [0], hint = '' } = {}) {
  const how = invocation(command, args);
  const result = spawnSync(how.file, how.args, {
    cwd: ROOT,
    encoding: 'utf8',
    shell: how.shell,
    maxBuffer: MAX_CAPTURED_BYTES,
  });
  if (result.error) {
    throw new GateError(`Could not start ${describe(command, args)}: ${result.error.message}. Check that ${command} is installed and on your PATH.`);
  }
  if (!allowed.includes(result.status)) {
    const detail = result.stderr.trim();
    throw new GateError(`${describe(command, args)} failed with status ${String(result.status)}${detail ? `:\n${detail}` : '.'}${hint ? `\n${hint}` : ''}`);
  }
  return { out: result.stdout, status: result.status };
}

const cargo = (...args) => run('cargo', args);
const pnpm = (...args) => run('pnpm', args);

function requireCargoTool(tool) {
  const subcommand = tool.replace(/^cargo-/, '');
  const probe = spawnSync('cargo', [subcommand, '--version'], { cwd: ROOT, encoding: 'utf8' });
  if (probe.status !== 0) {
    throw new GateError(`${tool} is not installed. Install the version CI uses with \`node scripts/gate.mjs --install-tools\`.`);
  }
}

function installTools() {
  for (const [tool, version] of Object.entries(TOOLS)) {
    cargo('install', tool, '--version', version, '--locked');
  }
}

/** Tauri embeds the built frontend at compile time. When it has not been built, a placeholder page lets clippy and the tests compile, and says what to do if someone runs that build. */
function ensureDesktopFrontend() {
  const config = JSON.parse(readFileSync(join(ROOT, TAURI_CONFIG), 'utf8'));
  const folder = resolve(ROOT, dirname(TAURI_CONFIG), config.build.frontendDist);
  if (existsSync(folder)) return;
  console.log(`The desktop interface is not built; writing a placeholder to ${relative(ROOT, folder)} so the desktop crate compiles. Run \`pnpm --filter @wirecatch/desktop build\` for the real interface.`);
  mkdirSync(folder, { recursive: true });
  writeFileSync(
    join(folder, 'index.html'),
    '<!doctype html><meta charset="utf-8"><title>Not built</title><p>The interface was not built. Run <code>pnpm --filter @wirecatch/desktop build</code>, then build the app again.</p>\n',
  );
}

// ---------------------------------------------------------------------------------------------
// Checks

function cargoMetadata({ deps = false } = {}) {
  const args = ['metadata', '--format-version', '1', '--locked'];
  if (!deps) args.push('--no-deps');
  return JSON.parse(capture('cargo', args).out);
}

/** Whether a manifest has `workspace = true` inside its `[lints]` table. */
function followsWorkspaceLints(manifest) {
  const table = /^\[lints\][^\S\r\n]*\r?\n([\s\S]*?)(?=^\[|(?![\s\S]))/m.exec(manifest);
  return table !== null && /^\s*workspace\s*=\s*true\s*(#.*)?$/m.test(table[1]);
}

function lintPolicy() {
  const problems = [];
  const names = new Set();
  for (const pkg of cargoMetadata().packages) {
    names.add(pkg.name);
    const optedIn = followsWorkspaceLints(readFileSync(pkg.manifest_path, 'utf8'));
    const pending = LINT_PENDING.has(pkg.name);
    const where = relative(ROOT, pkg.manifest_path);
    if (!optedIn && !pending) {
      problems.push(`${where}: add \`[lints] workspace = true\`; every crate follows the workspace lint policy.`);
    }
    if (optedIn && pending) {
      problems.push(`${where} follows the workspace lints now: remove ${pkg.name} from LINT_PENDING in scripts/gate.mjs.`);
    }
  }
  for (const name of LINT_PENDING) {
    if (!names.has(name)) problems.push(`${name} is in LINT_PENDING in scripts/gate.mjs but is not in the workspace; remove it.`);
  }
  if (problems.length > 0) throw new GateError(problems.join('\n'));
  if (LINT_PENDING.size > 0) {
    console.log(`Crates still to bring under the workspace lints (docs/AUDIT.md, W14): ${[...LINT_PENDING].join(', ')}`);
  }
}

function clippy() {
  if (options.desktop) ensureDesktopFrontend();
  const exclusion = options.desktop ? [] : ['--exclude', DESKTOP_CRATE];
  cargo('clippy', '--workspace', ...exclusion, '--all-targets', '--locked', ...TEST_FEATURES, '--', '-D', 'warnings');
  // The bridge as it ships: without the test-only feature, code behind `cfg(not(feature = "test-overrides"))` is linted too.
  cargo('clippy', '-p', 'wirecatch-bridge', '--all-targets', '--locked', '--', '-D', 'warnings');
}

function test() {
  const failFast = options.failFast ? [] : ['--no-fail-fast'];
  cargo('test', '--workspace', '--exclude', DESKTOP_CRATE, '--locked', ...failFast, ...TEST_FEATURES);
}

/** The desktop crate is tested on its own: its Tauri build script adds link paths that break other crates' doctests when everything links together. */
function testDesktop() {
  if (!options.desktop) return;
  ensureDesktopFrontend();
  cargo('test', '-p', DESKTOP_CRATE, '--locked', ...(options.failFast ? [] : ['--no-fail-fast']));
}

/** The published engine still builds with the oldest Rust its manifest promises. */
function msrv() {
  const engine = cargoMetadata().packages.find((p) => p.name === ENGINE_CRATE);
  const version = engine?.rust_version;
  if (!version) throw new GateError(`${ENGINE_CRATE} has no rust-version in its manifest; it is published and must state one.`);
  run('rustup', ['toolchain', 'install', version, '--profile', 'minimal']);
  cargo(`+${version}`, 'check', '-p', ENGINE_CRATE, '--locked');
}

const GENERATED_TYPES = 'packages/protocol/src';

/** Every file under `dir`, with its contents, keyed by its path from the repository root. */
function folderContents(dir) {
  const contents = new Map();
  const walk = (folder) => {
    for (const entry of readdirSync(folder, { withFileTypes: true })) {
      const path = join(folder, entry.name);
      if (entry.isDirectory()) walk(path);
      else contents.set(relative(ROOT, path), readFileSync(path, 'utf8'));
    }
  };
  walk(join(ROOT, dir));
  return contents;
}

/** Regenerates the TypeScript types and fails if that changed any file: what was there did not match `crates/protocol`. It compares the files themselves, so it also works before a change is committed. */
function protocolDrift() {
  const before = folderContents(GENERATED_TYPES);
  pnpm('protocol:generate');
  const after = folderContents(GENERATED_TYPES);
  const paths = new Set([...before.keys(), ...after.keys()]);
  const changed = [...paths].filter((path) => before.get(path) !== after.get(path));
  if (changed.length > 0) {
    throw new GateError(`The generated TypeScript protocol types were out of date; they are regenerated now, so review and commit them:\n${changed.join('\n')}`);
  }
}

/** Lines matching `pattern` in tracked and untracked (not ignored) text files; git decides what is binary. */
function grepRepository(pattern, pathspecs) {
  const { out } = capture('git', ['grep', '--untracked', '-I', '-n', '-P', pattern, '--', ...pathspecs], { allowed: [0, 1] });
  return out.split('\n').filter(Boolean);
}

function textRules() {
  const hits = [...grepRepository(EM_DASH_LITERAL, TEXT_SCOPE), ...grepRepository(EM_DASH_ESCAPED, ESCAPE_SCOPE)];
  if (hits.length > 0) {
    throw new GateError(`Em dashes are not used in this repository; use a comma, colon, parentheses or a new sentence:\n${hits.join('\n')}`);
  }
}

/** What a commit message says, without git's comment lines or the diff `git commit -v` appends. */
function messageText(raw) {
  const normalized = raw.replace(/\r\n?/g, '\n');
  const scissors = SCISSORS.exec(normalized);
  const kept = scissors ? normalized.slice(0, scissors.index) : normalized;
  return kept
    .split('\n')
    .filter((line) => !line.startsWith('#'))
    .join('\n')
    .trim();
}

/** Returns what is wrong with one commit message, or an empty list. With `subjectOnly`, the body is not checked. */
function commitProblems(raw, { subjectOnly = false } = {}) {
  const text = messageText(raw);
  const [subject = '', ...rest] = text.split('\n');
  if (AUTOSQUASH.test(subject)) return [];
  const problems = [];
  const pattern = new RegExp(`^(${COMMIT_TYPES.join('|')})(\\((${COMMIT_SCOPES.join('|')})\\))?!?: (.+)$`);
  const match = pattern.exec(subject);
  if (!match) {
    const revert = subject.startsWith('Revert "') ? ' For a revert, write `revert: <the original subject>`.' : '';
    problems.push(`the subject must look like \`type(scope): subject\`, with a type from ${COMMIT_TYPES.join(', ')} and a scope from ${COMMIT_SCOPES.join(', ')} (or no scope for a repository-wide change).${revert}`);
  } else {
    const words = match[4];
    if (/^[A-Z]/.test(words)) problems.push('the subject must start in lower case');
    if (words.endsWith('.')) problems.push('the subject must not end with a period');
  }
  if (subject.length > MAX_SUBJECT_LENGTH) {
    problems.push(`the subject is ${String(subject.length)} characters; the limit is ${String(MAX_SUBJECT_LENGTH)}`);
  }
  const checked = subjectOnly ? subject : text;
  if (checked.includes('\u2014')) problems.push('the message contains an em dash');
  const body = rest.join('\n');
  if (!subjectOnly && AUTHOR_CREDITS.some((credit) => credit.test(body))) {
    problems.push('the message credits a tool or AI as an author; remove that line');
  }
  return problems;
}

/** Merge commits are not checked: git writes their messages, and the `chore: merge` form for merges into main is a review rule. */
function commitRange() {
  const range = process.env.GATE_COMMIT_RANGE ?? 'origin/main..HEAD';
  const hint = 'Fetch the main branch (`git fetch origin main`) or set GATE_COMMIT_RANGE to the commits to check.';
  const shas = capture('git', ['rev-list', '--no-merges', range], { hint }).out.split('\n').filter(Boolean);
  const failures = [];
  for (const sha of shas) {
    const [author, ...message] = capture('git', ['log', '-1', '--format=%ae%n%B', sha]).out.split('\n');
    const problems = commitProblems(message.join('\n'), { subjectOnly: BOT_AUTHORS.test(author) });
    if (problems.length > 0) failures.push(`${sha.slice(0, 9)} ${message[0]}\n  - ${problems.join('\n  - ')}`);
  }
  if (failures.length > 0) throw new GateError(`Commit messages that break docs/ENGINEERING.md, section 9:\n${failures.join('\n')}`);
  console.log(`${String(shas.length)} commit message(s) in ${range} follow the rules.`);
}

function commitMessageFile(file) {
  if (!file) throw new GateError('commit-msg needs the path of the message file.');
  // A merge's message is written by git (see commitRange).
  const mergeHead = capture('git', ['rev-parse', '--git-path', 'MERGE_HEAD']).out.trim();
  if (existsSync(resolve(ROOT, mergeHead))) return;
  const problems = commitProblems(readFileSync(file, 'utf8'));
  if (problems.length > 0) throw new GateError(`This commit message breaks docs/ENGINEERING.md, section 9:\n  - ${problems.join('\n  - ')}`);
}

function majorMinor(version) {
  const match = /(\d+)\.(\d+)/.exec(version);
  return match ? `${match[1]}.${match[2]}` : version;
}

/** Compares the versions actually resolved (Cargo.lock and the installed npm packages), so a lockfile-only update cannot drift unnoticed. */
function tauriPairs() {
  const resolved = new Map();
  for (const pkg of cargoMetadata({ deps: true }).packages) {
    if (pkg.source !== null) resolved.set(pkg.name, [...(resolved.get(pkg.name) ?? []), pkg.version]);
  }
  const problems = [];
  for (const [crate, npm] of TAURI_PAIRS) {
    const rust = resolved.get(crate) ?? [];
    const manifest = join(ROOT, DESKTOP_APP, 'node_modules', npm, 'package.json');
    if (rust.length !== 1) {
      problems.push(`Expected one resolved version of ${crate}, found ${rust.length > 0 ? rust.join(', ') : 'none'}; update TAURI_PAIRS in scripts/gate.mjs if it is no longer used.`);
      continue;
    }
    if (!existsSync(manifest)) {
      problems.push(`${npm} is not installed in ${DESKTOP_APP}. Run \`pnpm install\` first, or update TAURI_PAIRS if it is no longer used.`);
      continue;
    }
    const js = JSON.parse(readFileSync(manifest, 'utf8')).version;
    if (majorMinor(rust[0]) !== majorMinor(js)) {
      problems.push(`${crate} ${rust[0]} and ${npm} ${js} must be on the same major.minor; update them together.`);
    }
  }
  if (problems.length > 0) throw new GateError(problems.join('\n'));
}

// ---------------------------------------------------------------------------------------------
// Registry

const CHECKS = {
  fmt: () => cargo('fmt', '--all', '--check'),
  'lint-policy': lintPolicy,
  clippy,
  'clippy-http3': () => cargo('clippy', '-p', ENGINE_CRATE, '-p', 'wirecatch-cli', '--all-targets', '--locked', '--features', 'wirecatch-engine/http3,wirecatch-cli/http3', '--', '-D', 'warnings'),
  test,
  // The engine with its off-by-default http3 feature, which no other check compiles into a test.
  'test-http3': () => cargo('test', '-p', ENGINE_CRATE, '--locked', '--features', 'http3'),
  'test-desktop': testDesktop,
  // With every feature, as docs.rs builds them (`package.metadata.docs.rs` in the engine's Cargo.toml).
  'engine-docs': () => run('cargo', ['doc', '-p', ENGINE_CRATE, '--no-deps', '--locked', '--all-features'], { env: { RUSTDOCFLAGS: '-D warnings' } }),
  // Verifies the package builds as published; git cleanliness is not its concern, so it also runs before a change is committed.
  'engine-package': () => cargo('package', '-p', ENGINE_CRATE, '--locked', '--allow-dirty'),
  msrv,
  protocol: protocolDrift,
  'js-check': () => pnpm('check'),
  'js-lint': () => pnpm('lint'),
  'js-test': () => pnpm('test'),
  'js-build': () => pnpm('build'),
  text: textRules,
  commits: commitRange,
  'tauri-pairs': tauriPairs,
  deny: () => {
    requireCargoTool('cargo-deny');
    cargo('deny', '--locked', 'check');
  },
  'unused-deps': () => {
    requireCargoTool('cargo-machete');
    cargo('machete');
  },
  // Ignored advisories live in `auditConfig` in pnpm-workspace.yaml. GHSA-86w9-cpqp-85rv (node-forge, no fixed release) reaches us only through web-ext's Android device support, which the Firefox signing script never uses.
  'npm-audit': () => pnpm('audit'),
};

const GROUPS = {
  // The http3 checks run here, on every system CI tests, because QUIC sockets behave differently on Windows.
  rust: ['fmt', 'lint-policy', 'clippy', 'test', 'test-desktop', 'clippy-http3', 'test-http3', 'engine-docs'],
  release: ['engine-package', 'msrv'],
  js: ['protocol', 'js-check', 'js-lint', 'js-test', 'js-build'],
  repo: ['text', 'tauri-pairs', 'commits'],
  'supply-chain': ['unused-deps', 'deny', 'npm-audit'],
};

/** What `node scripts/gate.mjs` runs with no names: every check CI runs. */
const DEFAULT = ['repo', 'rust', 'release', 'js', 'supply-chain'];

function expand(names) {
  return names.flatMap((name) => {
    if (GROUPS[name]) return expand(GROUPS[name]);
    if (CHECKS[name]) return [name];
    throw new GateError(`Unknown check or group "${name}". Run with --list to see them.`);
  });
}

function main(rawArgs) {
  const names = [];
  for (const arg of rawArgs) {
    if (arg === '--no-desktop') options.desktop = false;
    else if (arg === '--no-fail-fast') options.failFast = false;
    else names.push(arg);
  }
  if (!options.desktop) console.log(`Leaving out ${DESKTOP_CRATE} (--no-desktop). This run does not cover the desktop crate.`);
  if (names[0] === '--list') {
    console.log(`Checks: ${Object.keys(CHECKS).join(', ')}`);
    for (const [group, members] of Object.entries(GROUPS)) console.log(`${group}: ${members.join(', ')}`);
    console.log(`default: ${DEFAULT.join(', ')}`);
    return;
  }
  if (names[0] === '--install-tools') {
    installTools();
    return;
  }
  if (names[0] === 'commit-msg') {
    commitMessageFile(names[1]);
    return;
  }
  const checks = [...new Set(expand(names.length > 0 ? names : DEFAULT))];
  for (const name of checks) {
    console.log(`\n=== ${name}`);
    CHECKS[name]();
  }
  console.log(`\nGate passed: ${checks.join(', ')}`);
}

try {
  main(process.argv.slice(2));
} catch (error) {
  if (error instanceof GateError) {
    console.error(`\nGate failed.\n${error.message}`);
    process.exit(1);
  }
  throw error;
}
