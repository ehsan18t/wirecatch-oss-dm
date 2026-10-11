#!/usr/bin/env node
// The quality gate: every check a change must pass, in one place. CI, CLAUDE.md, CONTRIBUTING.md and scripts/linux-test.sh all run this script instead of repeating its commands, so they cannot drift apart.
//
//   node scripts/gate.mjs                 run every local check
//   node scripts/gate.mjs rust js         run named checks or groups (see GROUPS)
//   node scripts/gate.mjs commit-msg FILE check one commit message (the commit-msg hook)
//   node scripts/gate.mjs --list          list checks and groups
//   node scripts/gate.mjs --no-desktop …  leave out the desktop crate, for machines without its system libraries (it says so when it does)
//
// The commit range for `commits` defaults to `origin/main..HEAD`; set GATE_COMMIT_RANGE to override it.

import { execFileSync, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync } from 'node:fs';
import { dirname, join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');

/** Features every workspace-wide cargo command needs. `test-overrides` exists for the bridge's end-to-end tests only and is never enabled in a build. */
const TEST_FEATURES = ['--features', 'wirecatch-bridge/test-overrides'];

/**
 * Crates that do not yet opt into `[workspace.lints]`. The list only ever shrinks: a crate leaves it in the same change that makes it lint-clean (docs/AUDIT.md, W14). The check fails if a crate is missing from the list without opting in, or opts in while still listed.
 */
const LINT_PENDING = new Set([
  'wirecatch-bridge',
  'wirecatch-core',
  'wirecatch-desktop',
  'wirecatch-e2e',
  'wirecatch-engine',
  'wirecatch-ipc',
  'wirecatch-protocol',
  'wirecatch-streams',
]);

/** Conventional Commit types and package scopes allowed in commit subjects (CLAUDE.md). */
const COMMIT_TYPES = ['feat', 'fix', 'docs', 'test', 'refactor', 'perf', 'build', 'ci', 'chore', 'style', 'revert'];
const COMMIT_SCOPES = ['engine', 'protocol', 'core', 'ipc', 'bridge', 'streams', 'cli', 'e2e', 'desktop', 'extension'];
const MAX_SUBJECT_LENGTH = 72;
/** Lines that credit a tool or an AI as an author. Commits are authored by the person who owns the repository. */
const FORBIDDEN_TRAILERS = [/^co-authored-by:/im, /^generated (with|by)\b/im, /noreply@anthropic\.com/i];

const EM_DASH = '\u2014';
/** Tracked files the text check skips: lockfiles and binary formats. */
const TEXT_SKIP = /(^|\/)(Cargo\.lock|pnpm-lock\.yaml)$|\.(png|ico|icns|jpg|jpeg|gif|webp|woff2?|ttf|zip|pem|der|bin)$/i;

/** Rust and JS package pairs that must stay on the same major.minor (Tauri warns at build time when they differ). */
const TAURI_PAIRS = [
  ['tauri', '@tauri-apps/api'],
  ['tauri-plugin-dialog', '@tauri-apps/plugin-dialog'],
];

const DESKTOP_FRONTEND = 'apps/desktop/build';
const DESKTOP_CRATE = 'wirecatch-desktop';

/** Set by `--no-desktop`. */
let withDesktop = true;

/** `--exclude wirecatch-desktop` when the desktop crate is left out. */
const desktopExclusion = () => (withDesktop ? [] : ['--exclude', DESKTOP_CRATE]);

// ---------------------------------------------------------------------------------------------
// Running commands

class GateError extends Error {}

/** pnpm is a `.cmd` script on Windows and needs a shell; cargo and git are real executables and must not get one, so `%` and quotes in their arguments reach them unchanged. */
const needsShell = (command) => process.platform === 'win32' && command === 'pnpm';

function run(command, args, { env } = {}) {
  console.log(`\n$ ${[command, ...args].join(' ')}`);
  const result = spawnSync(command, args, {
    cwd: ROOT,
    stdio: 'inherit',
    shell: needsShell(command),
    env: { ...process.env, ...env },
  });
  if (result.status !== 0) {
    throw new GateError(`\`${command} ${args.join(' ')}\` failed with status ${String(result.status)}.`);
  }
}

function capture(command, args) {
  return execFileSync(command, args, {
    cwd: ROOT,
    encoding: 'utf8',
    shell: needsShell(command),
    maxBuffer: 64 * 1024 * 1024,
  });
}

const cargo = (...args) => run('cargo', args);
const pnpm = (...args) => run('pnpm', args);

/** Tauri embeds the built frontend at compile time; an empty folder is enough for clippy and tests. */
function ensureDesktopFrontend() {
  const folder = join(ROOT, DESKTOP_FRONTEND);
  if (!existsSync(folder)) {
    console.log(`Creating an empty ${DESKTOP_FRONTEND} so the desktop crate compiles. Run \`pnpm --filter @wirecatch/desktop build\` for the real one.`);
    mkdirSync(folder, { recursive: true });
  }
}

// ---------------------------------------------------------------------------------------------
// Checks

function cargoMetadata() {
  return JSON.parse(capture('cargo', ['metadata', '--format-version', '1', '--no-deps', '--locked']));
}

function lintPolicy() {
  const problems = [];
  for (const pkg of cargoMetadata().packages) {
    const manifest = readFileSync(pkg.manifest_path, 'utf8');
    const optedIn = /^\[lints\]\s*\r?\nworkspace\s*=\s*true/m.test(manifest);
    const pending = LINT_PENDING.has(pkg.name);
    const where = relative(ROOT, pkg.manifest_path);
    if (!optedIn && !pending) {
      problems.push(`${where}: add \`[lints] workspace = true\`; every crate follows the workspace lint policy.`);
    }
    if (optedIn && pending) {
      problems.push(`${where} follows the workspace lints now: remove ${pkg.name} from LINT_PENDING in scripts/gate.mjs.`);
    }
  }
  if (problems.length > 0) throw new GateError(problems.join('\n'));
  if (LINT_PENDING.size > 0) {
    console.log(`Crates still to bring under the workspace lints (docs/AUDIT.md, W14): ${[...LINT_PENDING].join(', ')}`);
  }
}

function protocolDrift() {
  pnpm('protocol:generate');
  const changes = capture('git', ['status', '--porcelain', '--', 'packages/protocol/src']).trim();
  if (changes) {
    throw new GateError(`The generated TypeScript protocol types are out of date. Run \`pnpm protocol:generate\` and commit the result:\n${changes}`);
  }
}

function textRules() {
  const files = capture('git', ['ls-files', '-z']).split('\0').filter((f) => f && !TEXT_SKIP.test(f));
  const hits = [];
  for (const file of files) {
    const path = join(ROOT, file);
    if (!existsSync(path)) continue;
    const lines = readFileSync(path, 'utf8').split('\n');
    lines.forEach((line, index) => {
      if (line.includes(EM_DASH)) hits.push(`${file}:${String(index + 1)}`);
    });
  }
  if (hits.length > 0) {
    throw new GateError(`Em dashes are not used in this repository; use a comma, colon, parentheses or a new sentence:\n${hits.join('\n')}`);
  }
}

/** Returns what is wrong with one commit message, or an empty list. */
function commitProblems(message) {
  const lines = message.split('\n').filter((line) => !line.startsWith('#'));
  const subject = lines[0] ?? '';
  const problems = [];
  const pattern = new RegExp(`^(${COMMIT_TYPES.join('|')})(\\((${COMMIT_SCOPES.join('|')})\\))?!?: (.+)$`);
  const match = pattern.exec(subject);
  if (!match) {
    problems.push(`the subject must look like \`type(scope): subject\`, with a type from ${COMMIT_TYPES.join(', ')} and a scope from ${COMMIT_SCOPES.join(', ')} (or no scope for a repo-wide change)`);
  } else {
    const text = match[4];
    if (/^[A-Z]/.test(text)) problems.push('the subject must start in lower case');
    if (text.endsWith('.')) problems.push('the subject must not end with a period');
  }
  if (subject.length > MAX_SUBJECT_LENGTH) {
    problems.push(`the subject is ${String(subject.length)} characters; the limit is ${String(MAX_SUBJECT_LENGTH)}`);
  }
  if (message.includes(EM_DASH)) problems.push('the message contains an em dash');
  const body = lines.slice(1).join('\n');
  if (FORBIDDEN_TRAILERS.some((pattern) => pattern.test(body))) {
    problems.push('the message credits a tool or AI as an author; remove that line');
  }
  return problems;
}

function commitRange() {
  const range = process.env.GATE_COMMIT_RANGE ?? 'origin/main..HEAD';
  const shas = capture('git', ['rev-list', '--no-merges', range]).split('\n').filter(Boolean);
  const failures = [];
  for (const sha of shas) {
    const message = capture('git', ['log', '-1', '--format=%B', sha]);
    const problems = commitProblems(message);
    if (problems.length > 0) failures.push(`${sha.slice(0, 9)} ${message.split('\n')[0]}\n  - ${problems.join('\n  - ')}`);
  }
  if (failures.length > 0) throw new GateError(`Commit messages that break the rules in CLAUDE.md:\n${failures.join('\n')}`);
  console.log(`${String(shas.length)} commit message(s) in ${range} follow the rules.`);
}

function commitMessageFile(file) {
  if (!file) throw new GateError('commit-msg needs the path of the message file.');
  const problems = commitProblems(readFileSync(file, 'utf8'));
  if (problems.length > 0) throw new GateError(`This commit message breaks the rules in CLAUDE.md:\n  - ${problems.join('\n  - ')}`);
}

function majorMinor(version) {
  const match = /(\d+)\.(\d+)/.exec(version);
  return match ? `${match[1]}.${match[2]}` : version;
}

function tauriPairs() {
  const desktop = cargoMetadata().packages.find((p) => p.name === 'wirecatch-desktop');
  if (!desktop) throw new GateError('The wirecatch-desktop crate is missing from the workspace.');
  const rust = new Map(desktop.dependencies.map((d) => [d.name, d.req]));
  const pkg = JSON.parse(readFileSync(join(ROOT, 'apps/desktop/package.json'), 'utf8'));
  const js = { ...pkg.dependencies, ...pkg.devDependencies };
  const problems = [];
  for (const [crate, npm] of TAURI_PAIRS) {
    const rustVersion = rust.get(crate);
    const jsVersion = js[npm];
    if (!rustVersion || !jsVersion) {
      problems.push(`${crate} or ${npm} is no longer a dependency; update TAURI_PAIRS in scripts/gate.mjs.`);
    } else if (majorMinor(rustVersion) !== majorMinor(jsVersion)) {
      problems.push(`${crate} ${rustVersion} and ${npm} ${jsVersion} must be on the same major.minor; update them together.`);
    }
  }
  if (problems.length > 0) throw new GateError(problems.join('\n'));
}

// ---------------------------------------------------------------------------------------------
// Registry

const CHECKS = {
  fmt: () => cargo('fmt', '--all', '--check'),
  'lint-policy': lintPolicy,
  clippy: () => {
    if (withDesktop) ensureDesktopFrontend();
    cargo('clippy', '--workspace', ...desktopExclusion(), '--all-targets', '--locked', ...TEST_FEATURES, '--', '-D', 'warnings');
  },
  'clippy-http3': () => cargo('clippy', '-p', 'wirecatch-engine', '-p', 'wirecatch-cli', '--all-targets', '--locked', '--features', 'wirecatch-engine/http3,wirecatch-cli/http3', '--', '-D', 'warnings'),
  test: () => cargo('test', '--workspace', '--exclude', DESKTOP_CRATE, '--locked', ...TEST_FEATURES),
  // The desktop crate is tested on its own: its Tauri build script adds link paths that break other crates' doctests when everything links together.
  'test-desktop': () => {
    if (!withDesktop) return;
    ensureDesktopFrontend();
    cargo('test', '-p', DESKTOP_CRATE, '--locked');
  },
  'engine-docs': () => run('cargo', ['doc', '-p', 'wirecatch-engine', '--no-deps', '--locked'], { env: { RUSTDOCFLAGS: '-D warnings' } }),
  'engine-package': () => cargo('package', '-p', 'wirecatch-engine', '--locked'),
  protocol: protocolDrift,
  'js-check': () => pnpm('check'),
  'js-lint': () => pnpm('lint'),
  'js-test': () => pnpm('test'),
  'js-build': () => pnpm('build'),
  text: textRules,
  commits: commitRange,
  'tauri-pairs': tauriPairs,
  deny: () => cargo('deny', '--locked', 'check'),
  'unused-deps': () => cargo('machete'),
};

const GROUPS = {
  rust: ['fmt', 'lint-policy', 'clippy', 'test', 'test-desktop', 'engine-docs'],
  js: ['protocol', 'js-check', 'js-lint', 'js-test', 'js-build'],
  repo: ['text', 'tauri-pairs'],
  release: ['engine-package'],
  'supply-chain': ['unused-deps', 'deny'],
};

/** What `node scripts/gate.mjs` runs with no arguments: everything a change must pass before it is reported done. */
const DEFAULT = ['repo', 'rust', 'js'];

function expand(names) {
  return names.flatMap((name) => {
    if (GROUPS[name]) return expand(GROUPS[name]);
    if (CHECKS[name]) return [name];
    throw new GateError(`Unknown check or group "${name}". Run with --list to see them.`);
  });
}

function main(rawArgs) {
  const argv = rawArgs.filter((arg) => arg !== '--no-desktop');
  if (argv.length !== rawArgs.length) {
    withDesktop = false;
    console.log(`Leaving out ${DESKTOP_CRATE} (--no-desktop). This run does not cover the desktop crate.`);
  }
  if (argv[0] === '--list') {
    console.log(`Checks: ${Object.keys(CHECKS).join(', ')}`);
    for (const [group, members] of Object.entries(GROUPS)) console.log(`${group}: ${members.join(', ')}`);
    console.log(`default: ${DEFAULT.join(', ')}`);
    return;
  }
  if (argv[0] === 'commit-msg') {
    commitMessageFile(argv[1]);
    return;
  }
  const checks = [...new Set(expand(argv.length > 0 ? argv : DEFAULT))];
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
