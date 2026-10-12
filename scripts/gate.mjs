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

/** Cargo tools some checks run, with the versions CI installs (`--install-tools`). */
const TOOLS = {
  'cargo-deny': '0.19.0',
  'cargo-machete': '0.9.2',
};

/** Conventional Commit types and package scopes allowed in commit subjects (docs/ENGINEERING.md, section 9). */
const COMMIT_TYPES = ['feat', 'fix', 'docs', 'test', 'refactor', 'perf', 'build', 'ci', 'chore', 'style', 'revert'];
const COMMIT_SCOPES = ['engine', 'protocol', 'core', 'ipc', 'bridge', 'streams', 'cli', 'e2e', 'test-support', 'desktop', 'extension', 'units', 'eslint-config'];
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
const BRIDGE_CRATE = 'wirecatch-bridge';

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

/** Every crate opts into `[workspace.lints]`, so no crate can quietly follow a weaker policy. */
function lintPolicy() {
  const problems = [];
  for (const pkg of cargoMetadata().packages) {
    if (!followsWorkspaceLints(readFileSync(pkg.manifest_path, 'utf8'))) {
      const where = relative(ROOT, pkg.manifest_path);
      problems.push(`${where}: add \`[lints] workspace = true\`; every crate follows the workspace lint policy.`);
    }
  }
  if (problems.length > 0) throw new GateError(problems.join('\n'));
}

function clippy() {
  if (options.desktop) ensureDesktopFrontend();
  const exclusion = options.desktop ? [] : ['--exclude', DESKTOP_CRATE];
  cargo('clippy', '--workspace', ...exclusion, '--all-targets', '--locked', ...TEST_FEATURES, '--', '-D', 'warnings');
  // The bridge as it ships: without the test-only feature, code behind `cfg(not(feature = "test-overrides"))` is linted too.
  cargo('clippy', '-p', BRIDGE_CRATE, '--all-targets', '--locked', '--', '-D', 'warnings');
}

/** The system Linux CI checks, linted from other systems by `clippy-linux`. */
const LINUX_TARGET = 'x86_64-unknown-linux-gnu';
/** A package in a crate's dependency tree that compiles C for the target (ring and bundled SQLite build with it), which needs a C compiler for Linux. */
const C_COMPILER_CRATE = /^cc v/m;

/**
 * The workspace crates that can be linted for Linux without a C compiler for Linux: those whose dependency tree for Linux, dev-dependencies included (`--all-targets`), never compiles C. `cargo tree` resolves each crate's features on its own, as the lint run does. The list grows by itself as crates stop depending on ring and other C code.
 */
function linuxLintableCrates() {
  return cargoMetadata()
    .packages.map((pkg) => pkg.name)
    .filter((name) => {
      const args = ['tree', '-p', name, '--target', LINUX_TARGET, '--edges', 'normal,build,dev', '--prefix', 'none', '--format', '{p}', '--locked'];
      return !C_COMPILER_CRATE.test(capture('cargo', args).out);
    });
}

/**
 * Lints for Linux from Windows and macOS, so code behind `cfg(unix)` or `cfg(target_os = "linux")` is checked before Linux CI sees it. On Linux the `clippy` check already covers it. Crates that compile C for their target cannot be checked this way without a C cross-compiler, so only the crates that do not are (see linuxLintableCrates).
 */
function clippyLinux() {
  if (process.platform === 'linux') {
    console.log('Skipped on Linux, where the clippy check already lints for Linux.');
    return;
  }
  const installed = capture('rustup', ['target', 'list', '--installed']).out.split(/\r?\n/);
  if (!installed.includes(LINUX_TARGET)) {
    throw new GateError(`The ${LINUX_TARGET} target is not installed, so code for Linux cannot be linted here. Add it with \`rustup target add ${LINUX_TARGET}\` (only its standard library; no C compiler is needed).`);
  }
  const crates = linuxLintableCrates();
  if (crates.length === 0) throw new GateError('No workspace crate can be linted for Linux from this system; every one compiles C.');
  console.log(`Linting for ${LINUX_TARGET}: ${crates.join(', ')}.`);
  const packages = crates.flatMap((name) => ['-p', name]);
  const target = ['--target', LINUX_TARGET];
  const features = crates.includes(BRIDGE_CRATE) ? TEST_FEATURES : [];
  cargo('clippy', ...target, ...packages, '--all-targets', '--locked', ...features, '--', '-D', 'warnings');
  if (crates.includes(BRIDGE_CRATE)) cargo('clippy', ...target, '-p', BRIDGE_CRATE, '--all-targets', '--locked', '--', '-D', 'warnings');
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

/** Folders the published engine must not ship: its tests and examples use the unpublished test-support crate, so they cannot build from the package. */
const UNPACKAGED_ENGINE_FOLDERS = ['tests/', 'examples/'];

/** Builds the engine as published, then checks the package ships the library only (`include` in its Cargo.toml). */
function enginePackage() {
  cargo('package', '-p', ENGINE_CRATE, '--locked', '--allow-dirty');
  const files = capture('cargo', ['package', '-p', ENGINE_CRATE, '--list', '--locked', '--allow-dirty'])
    .out.split(/\r?\n/)
    .map((line) => line.trim().replaceAll('\\', '/'))
    .filter(Boolean);
  const shipped = files.filter((file) => UNPACKAGED_ENGINE_FOLDERS.some((folder) => file.startsWith(folder)));
  if (shipped.length > 0) {
    throw new GateError(`The ${ENGINE_CRATE} package must ship the library only, but it lists:\n${shipped.join('\n')}\nKeep \`include\` in crates/engine/Cargo.toml to the library's files.`);
  }
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

/** Sources that make a package subject to the shared ESLint configuration. */
const LINTED_SOURCE = /\.(ts|svelte)$/;
const ESLINT_CONFIG_PACKAGE = '@wirecatch/eslint-config';
/** A lint script that lints the whole package with no warnings allowed, then checks its formatting. */
const LINT_SCRIPT = /\beslint \. .*--max-warnings 0\b.*&& prettier --check \.$/;

/** The workspace package folders, from the `packages` globs in pnpm-workspace.yaml (each a `folder/*`). */
function workspacePackages() {
  const manifest = readFileSync(join(ROOT, 'pnpm-workspace.yaml'), 'utf8');
  const list = /^packages:\s*\n((?:\s+-\s+.*\n)+)/m.exec(manifest);
  if (!list) throw new GateError('pnpm-workspace.yaml has no `packages` list.');
  const globs = [...list[1].matchAll(/-\s+["']?([^"'\s]+)["']?/g)].map((m) => m[1]);
  return globs.flatMap((glob) => {
    if (!glob.endsWith('/*')) throw new GateError(`The workspace glob ${glob} is not of the form folder/*; teach workspacePackages in scripts/gate.mjs to read it.`);
    const parent = glob.slice(0, -2);
    return readdirSync(join(ROOT, parent), { withFileTypes: true })
      .filter((entry) => entry.isDirectory() && existsSync(join(ROOT, parent, entry.name, 'package.json')))
      .map((entry) => `${parent}/${entry.name}`);
  });
}

/** What is wrong with how the package in `folder` is linted, or an empty list. */
function jsPolicyProblems(folder) {
  const sources = capture('git', ['ls-files', '--cached', '--others', '--exclude-standard', '--', folder]).out.split('\n');
  if (!sources.some((file) => LINTED_SOURCE.test(file))) return [];
  const manifest = JSON.parse(readFileSync(join(ROOT, folder, 'package.json'), 'utf8'));
  const problems = [];
  const configPath = join(ROOT, folder, 'eslint.config.js');
  const config = existsSync(configPath) ? readFileSync(configPath, 'utf8') : '';
  const source = manifest.name === ESLINT_CONFIG_PACKAGE ? './index.js' : ESLINT_CONFIG_PACKAGE;
  if (!config.includes(`from '${source}'`) || !/\bwirecatch\(/.test(config)) {
    problems.push(`${folder}: eslint.config.js must build its configuration with \`wirecatch()\` from '${source}'.`);
  }
  if (!LINT_SCRIPT.test(manifest.scripts?.lint ?? '')) {
    problems.push(`${folder}: package.json needs \`"lint": "eslint . --max-warnings 0 && prettier --check ."\`, so \`pnpm lint\` lints and format-checks it.`);
  }
  return problems;
}

/** Every workspace package with TypeScript or Svelte sources is linted with the shared configuration, allows no warnings, and is format-checked. */
function jsPolicy() {
  const problems = workspacePackages().flatMap(jsPolicyProblems);
  if (problems.length > 0) throw new GateError(problems.join('\n'));
}

/** Lines matching `pattern` in tracked and untracked (not ignored) text files; git decides what is binary. */
function grepRepository(pattern, pathspecs) {
  const { out } = capture('git', ['grep', '--untracked', '-I', '-n', '-P', pattern, '--', ...pathspecs], { allowed: [0, 1] });
  return out.split('\n').filter(Boolean);
}

/** Files that may hold disable comments: everything ESLint and Svelte read, except this script, whose patterns name them. */
const COMMENT_SCOPE = ['*.ts', '*.js', '*.mjs', '*.cjs', '*.svelte', ':!scripts/gate.mjs'];
/** An ESLint disable comment in any of its forms, with the rules and reason that follow it. */
const ESLINT_DISABLE = /(?:\/\/|\/\*|<!--)\s*eslint-disable(?:-next-line|-line)?(?=\s|\*\/|-->|$)(.*?)(?:\*\/|-->|$)/gm;
/** A svelte-ignore comment, in markup or in a script, with the codes and reason that follow it. */
const SVELTE_IGNORE = /(?:<!--|\/\/|\/\*)\s*svelte-ignore\s(.*?)(?:-->|\*\/|$)/gm;
/** Svelte reads the codes, separated by commas, from the start of a svelte-ignore comment and treats what follows as prose. */
const SVELTE_IGNORE_CODES = /^\s*[\w$-]+(?:\s*,\s*[\w$-]+)*/;
/** Where a comment's reason starts: ESLint's `--` separator, which svelte-ignore comments use too. */
const REASON_SEPARATOR = /(?:^|\s)--+\s+\S/;

/** Every ESLint disable comment and svelte-ignore comment: the file and line it is on, what it switches off, and whether it gives a reason. */
function disableComments() {
  const files = new Set(grepRepository('eslint-disable|svelte-ignore', COMMENT_SCOPE).map((hit) => hit.slice(0, hit.indexOf(':'))));
  const found = [];
  for (const file of files) {
    const text = readFileSync(join(ROOT, file), 'utf8');
    const lineOf = (index) => text.slice(0, index).split('\n').length;
    for (const match of text.matchAll(ESLINT_DISABLE)) {
      const [rules] = match[1].split(/\s--+\s/);
      const lints = rules.split(',').map((rule) => rule.trim()).filter(Boolean);
      found.push({ file, line: lineOf(match.index), kind: 'eslint-disable', lints: lints.length > 0 ? lints.sort() : ['(every rule)'], reason: REASON_SEPARATOR.test(match[1]) });
    }
    for (const match of text.matchAll(SVELTE_IGNORE)) {
      const codes = SVELTE_IGNORE_CODES.exec(match[1])?.[0] ?? '';
      const lints = codes.split(',').map((code) => code.trim()).filter(Boolean);
      found.push({ file, line: lineOf(match.index), kind: 'svelte-ignore', lints: lints.sort(), reason: REASON_SEPARATOR.test(match[1].slice(codes.length).trim()) });
    }
  }
  return found;
}

function textRules() {
  const hits = [...grepRepository(EM_DASH_LITERAL, TEXT_SCOPE), ...grepRepository(EM_DASH_ESCAPED, ESCAPE_SCOPE)];
  if (hits.length > 0) {
    throw new GateError(`Em dashes are not used in this repository; use a comma, colon, parentheses or a new sentence:\n${hits.join('\n')}`);
  }
  // ESLint's own comments are held to a reason by the shared config (eslint-comments/require-description); Svelte's are not linted, so they are checked here.
  const unreasoned = disableComments().filter((c) => c.kind === 'svelte-ignore' && !c.reason);
  if (unreasoned.length > 0) {
    const list = unreasoned.map((c) => `${c.file}:${String(c.line)}: svelte-ignore ${c.lints.join(', ')}`).join('\n');
    throw new GateError(`A svelte-ignore comment says why the warning is wrong there, after \`--\` (\`<!-- svelte-ignore a11y_x -- the reason -->\`). Fix the cause where you can:\n${list}`);
  }
}

/**
 * The only places a lint is switched off, each listed with its reason in docs/AUDIT.md W14. An entry is one attribute or comment, named by its file and exactly the lints it switches off, so it may move within its file; a new one, another lint, or a second one in the same file fails the `exceptions` check until it is reasoned about and listed here and in AUDIT.
 */
const LINT_EXCEPTIONS = [
  { id: 'E1', file: 'apps/desktop/src/lib/backend/mock/rng.ts', lints: ['wirecatch/no-magic-numbers'] },
  { id: 'E2', file: 'crates/protocol/src/wire.rs', lints: ['clippy::large_enum_variant'] },
  { id: 'E3', file: 'crates/engine/src/transport.rs', lints: ['clippy::large_enum_variant'] },
  { id: 'E4', file: 'crates/protocol/src/app.rs', lints: ['clippy::struct_excessive_bools'] },
  { id: 'E5', file: 'crates/e2e/src/lib.rs', lints: ['clippy::expect_used', 'clippy::missing_panics_doc'] },
  { id: 'E6', file: 'crates/test-support/src/lib.rs', lints: ['clippy::missing_panics_doc', 'clippy::unwrap_used'] },
  { id: 'E7', file: 'crates/engine/src/numeric.rs', lints: ['clippy::cast_precision_loss'] },
  { id: 'E8', file: 'crates/engine/src/numeric.rs', lints: ['clippy::cast_possible_truncation', 'clippy::cast_sign_loss'] },
];

/** Every `#[expect]`, `#[allow]`, ESLint disable comment and svelte-ignore comment is one of LINT_EXCEPTIONS, and every listed exception is still in the code. */
function lintExceptions() {
  const unused = [...LINT_EXCEPTIONS];
  const unlisted = [];
  for (const site of [...rustLintAttributes(), ...disableComments()]) {
    const lints = site.lints.join(', ');
    const index = unused.findIndex((e) => e.file === site.file && [...e.lints].sort().join(', ') === lints);
    if (index === -1) unlisted.push(`${site.file}:${String(site.line)}: ${site.kind} ${lints}`);
    else unused.splice(index, 1);
  }
  const messages = [];
  if (unlisted.length > 0) {
    messages.push(`Lints switched off outside the listed exceptions. Fix the cause; only where the lint is wrong for that code, keep one exception with its reason and list it in LINT_EXCEPTIONS in scripts/gate.mjs and in docs/AUDIT.md W14:\n${unlisted.join('\n')}`);
  }
  if (unused.length > 0) {
    const list = unused.map((e) => `${e.id}: ${e.file}: ${e.lints.join(', ')}`).join('\n');
    messages.push(`Listed exceptions no longer in the code; remove them from LINT_EXCEPTIONS in scripts/gate.mjs and from docs/AUDIT.md W14:\n${list}`);
  }
  if (messages.length > 0) throw new GateError(messages.join('\n\n'));
}

// ---------------------------------------------------------------------------------------------
// Lint exceptions

/** Splits `text` at the commas outside brackets and string literals. */
function splitTopLevel(text) {
  const parts = [];
  let depth = 0;
  let start = 0;
  for (let i = 0; i < text.length; i += 1) {
    const char = text[i];
    if (char === '"') i = stringEnd(text, i);
    else if ('([{'.includes(char)) depth += 1;
    else if (')]}'.includes(char)) depth -= 1;
    else if (char === ',' && depth === 0) {
      parts.push(text.slice(start, i).trim());
      start = i + 1;
    }
  }
  parts.push(text.slice(start).trim());
  return parts.filter(Boolean);
}

/** The index of the quote that closes the Rust string opening at `open`. */
function stringEnd(text, open) {
  for (let i = open + 1; i < text.length; i += 1) {
    if (text[i] === '\\') i += 1;
    else if (text[i] === '"') return i;
  }
  return text.length;
}

/** The index just past the bracket that closes the one at `open`, skipping string literals. */
function closingBracket(text, open) {
  let depth = 0;
  for (let i = open; i < text.length; i += 1) {
    const char = text[i];
    if (char === '"') i = stringEnd(text, i);
    else if ('([{'.includes(char)) depth += 1;
    else if (')]}'.includes(char)) {
      depth -= 1;
      if (depth === 0) return i + 1;
    }
  }
  return text.length;
}

/** `name(args)` split into its name and its top-level arguments, or null for anything else. */
function attributeCall(text) {
  const match = /^([\w:]+)\s*\(/.exec(text);
  if (!match) return null;
  const open = match[0].length - 1;
  return { name: match[1], args: splitTopLevel(text.slice(open + 1, closingBracket(text, open) - 1)) };
}

/** The lints an `allow(...)` or `expect(...)` names, without its `reason`. */
function lintNames(args) {
  return args.filter((arg) => !/^reason\s*=/.test(arg)).sort();
}

/**
 * Every lint attribute in the Rust sources: each `#[allow]` and `#[expect]`, and each one a `#[cfg_attr]` switches on, with the file and line it is on and the lints it names. Lines that are comments are skipped, so docs may show the syntax.
 */
function rustLintAttributes() {
  const files = capture('git', ['grep', '-l', '--untracked', '-I', '-E', '(allow|expect|cfg_attr)[[:space:]]*\\(', '--', '*.rs'], { allowed: [0, 1] })
    .out.split('\n')
    .filter(Boolean);
  const found = [];
  for (const file of files) {
    const text = readFileSync(join(ROOT, file), 'utf8');
    for (const start of text.matchAll(/#!?\[/g)) {
      const lineStart = text.lastIndexOf('\n', start.index) + 1;
      if (text.slice(lineStart, start.index).includes('//')) continue;
      const open = start.index + start[0].length - 1;
      const call = attributeCall(text.slice(open + 1, closingBracket(text, open) - 1).trim());
      if (call === null) continue;
      const line = text.slice(0, start.index).split('\n').length;
      const nested = call.name === 'cfg_attr' ? call.args.slice(1).map(attributeCall).filter(Boolean) : [call];
      for (const attribute of nested) {
        if (attribute.name === 'allow' || attribute.name === 'expect') {
          found.push({ file, line, kind: attribute.name, conditional: call.name === 'cfg_attr', lints: lintNames(attribute.args) });
        }
      }
    }
  }
  return found;
}

/**
 * `#[allow]` is already an error through clippy's `allow_attributes`, but clippy only sees the attributes that are switched on for the system it runs on: `#[cfg_attr(not(windows), allow(...))]` passes on Windows and fails on Linux. This finds them on every system.
 */
function conditionalAllows() {
  const hits = rustLintAttributes().filter((a) => a.conditional && a.kind === 'allow');
  if (hits.length > 0) {
    const list = hits.map((a) => `${a.file}:${String(a.line)}: allow(${a.lints.join(', ')})`).join('\n');
    throw new GateError(`\`allow\` inside \`cfg_attr\` is not used: clippy cannot check it on the systems where it is switched off. Fix the cause (for code unused on one system, put it behind the same \`cfg\` as its users), or write \`#[expect(..., reason = "...")]\` and list it as an exception:\n${list}`);
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
  'cfg-allow': conditionalAllows,
  clippy,
  'clippy-linux': clippyLinux,
  'clippy-http3': () => cargo('clippy', '-p', ENGINE_CRATE, '-p', 'wirecatch-cli', '--all-targets', '--locked', '--features', 'wirecatch-engine/http3,wirecatch-cli/http3', '--', '-D', 'warnings'),
  test,
  // The engine with its off-by-default http3 feature, which no other check compiles into a test.
  'test-http3': () => cargo('test', '-p', ENGINE_CRATE, '--locked', '--features', 'http3'),
  'test-desktop': testDesktop,
  // With every feature, as docs.rs builds them (`package.metadata.docs.rs` in the engine's Cargo.toml).
  'engine-docs': () => run('cargo', ['doc', '-p', ENGINE_CRATE, '--no-deps', '--locked', '--all-features'], { env: { RUSTDOCFLAGS: '-D warnings' } }),
  // Verifies the package builds as published and ships the library only; git cleanliness is not its concern, so it also runs before a change is committed.
  'engine-package': enginePackage,
  msrv,
  protocol: protocolDrift,
  'js-policy': jsPolicy,
  'js-check': () => pnpm('check'),
  'js-lint': () => pnpm('lint'),
  'js-test': () => pnpm('test'),
  'js-build': () => pnpm('build'),
  text: textRules,
  exceptions: lintExceptions,
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
  rust: ['fmt', 'lint-policy', 'cfg-allow', 'clippy', 'clippy-linux', 'test', 'test-desktop', 'clippy-http3', 'test-http3', 'engine-docs'],
  release: ['engine-package', 'msrv'],
  js: ['js-policy', 'protocol', 'js-check', 'js-lint', 'js-test', 'js-build'],
  repo: ['text', 'exceptions', 'tauri-pairs', 'commits'],
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
