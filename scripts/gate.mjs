#!/usr/bin/env node
// The quality gate: every check a change must pass, in one place. CI, CLAUDE.md, CONTRIBUTING.md and scripts/linux-test.sh run this script instead of repeating its commands, so they cannot drift apart. This file is the runner; the checks live in scripts/gate/, one module per subject.
//
//   node scripts/gate.mjs                  every check CI runs (see DEFAULT)
//   node scripts/gate.mjs rust js          named checks or groups (see GROUPS)
//   node scripts/gate.mjs commit-msg FILE  check one commit message (the commit-msg hook)
//   node scripts/gate.mjs --install-tools  install the cargo tools some checks need, at the versions in TOOLS (scripts/gate/rust.mjs)
//   node scripts/gate.mjs --list           list checks and groups
//
// Options, before or after the names:
//   --no-desktop    leave out the desktop crate, for machines without its system libraries (it says so when it does)
//   --no-fail-fast  run every Rust test binary even after one fails
//
// The commit range for `commits` is GATE_COMMIT_RANGE when set, otherwise `origin/main..HEAD`. Written for Node 18 and later, so it also runs where only an older Node is installed (scripts/linux-test.sh).

import { COMMIT_CHECKS, commitMessageFile } from './gate/commits.mjs';
import { EXCEPTION_CHECKS } from './gate/exceptions.mjs';
import { JS_CHECKS } from './gate/javascript.mjs';
import { clippyLinux } from './gate/linux.mjs';
import { GateError, options } from './gate/process.mjs';
import { DESKTOP_CRATE, installTools, RUST_CHECKS } from './gate/rust.mjs';
import { TAURI_CHECKS } from './gate/tauri.mjs';
import { TEXT_CHECKS } from './gate/text.mjs';

const CHECKS = {
  ...RUST_CHECKS,
  ...EXCEPTION_CHECKS,
  'clippy-linux': clippyLinux,
  ...JS_CHECKS,
  ...TEXT_CHECKS,
  ...COMMIT_CHECKS,
  ...TAURI_CHECKS,
};

const GROUPS = {
  // The http3 checks run here, on every system CI tests, because QUIC sockets behave differently on Windows.
  rust: [
    'fmt',
    'lint-policy',
    'cfg-allow',
    'clippy',
    'clippy-linux',
    'test',
    'test-desktop',
    'clippy-http3',
    'test-http3',
    'engine-docs',
  ],
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

function list() {
  console.log(`Checks: ${Object.keys(CHECKS).join(', ')}`);
  for (const [group, members] of Object.entries(GROUPS))
    console.log(`${group}: ${members.join(', ')}`);
  console.log(`default: ${DEFAULT.join(', ')}`);
}

/** The commands that are not checks: they do one thing and stop. */
const COMMANDS = {
  '--list': list,
  '--install-tools': installTools,
  'commit-msg': commitMessageFile,
};

async function main(rawArgs) {
  const names = [];
  for (const arg of rawArgs) {
    if (arg === '--no-desktop') options.desktop = false;
    else if (arg === '--no-fail-fast') options.failFast = false;
    else names.push(arg);
  }
  if (!options.desktop)
    console.log(
      `Leaving out ${DESKTOP_CRATE} (--no-desktop). This run does not cover the desktop crate.`,
    );
  const command = COMMANDS[names[0]];
  if (command) {
    command(names[1]);
    return;
  }
  const checks = [...new Set(expand(names.length > 0 ? names : DEFAULT))];
  for (const name of checks) {
    console.log(`\n=== ${name}`);
    // Checks that need a module only some systems install (the ESLint API) are async and load it when they run.
    await CHECKS[name]();
  }
  console.log(`\nGate passed: ${checks.join(', ')}`);
}

try {
  await main(process.argv.slice(2));
} catch (error) {
  if (error instanceof GateError) {
    console.error(`\nGate failed.\n${error.message}`);
    process.exit(1);
  }
  throw error;
}
