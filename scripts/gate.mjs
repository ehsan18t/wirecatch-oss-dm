#!/usr/bin/env node
// The quality gate: every check a change must pass, in one place. CI, CLAUDE.md, CONTRIBUTING.md and scripts/linux-test.sh run this script instead of repeating its commands, so they cannot drift apart. This file is the runner; the checks live in scripts/gate/, one module per subject.
//
//   node scripts/gate.mjs                    every check CI runs (see DEFAULT)
//   node scripts/gate.mjs rust js            named checks or groups (see GROUPS)
//   node scripts/gate.mjs publish            what a release needs on top of DEFAULT
//   node scripts/gate.mjs publish-extension  `publish` plus what a release of the browser extension needs
//   node scripts/gate.mjs commit-msg FILE    check one commit message (the commit-msg hook)
//   node scripts/gate.mjs --install-tools    install the cargo tools some checks need, at the versions in TOOLS (scripts/gate/rust.mjs)
//   node scripts/gate.mjs --tool-specs       print those tools as name@version, for CI's prebuilt installer
//   node scripts/gate.mjs --list             list checks and groups
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
import { DESKTOP_CRATE, installTools, RUST_CHECKS, toolSpecs } from './gate/rust.mjs';
import { SUFFIX_LIST_CHECKS } from './gate/suffix-list.mjs';
import { TAURI_CHECKS } from './gate/tauri.mjs';
import { TEXT_CHECKS } from './gate/text.mjs';

/** A product check, its module loaded only when it runs: the generator behind it imports Prettier and smol-toml. */
function productCheck(name) {
  return async () => {
    const { PRODUCT_CHECKS } = await import('./gate/product.mjs');
    await PRODUCT_CHECKS[name]();
  };
}

const CHECKS = {
  ...RUST_CHECKS,
  ...EXCEPTION_CHECKS,
  'clippy-linux': clippyLinux,
  ...JS_CHECKS,
  ...TEXT_CHECKS,
  ...COMMIT_CHECKS,
  ...TAURI_CHECKS,
  ...SUFFIX_LIST_CHECKS,
  product: productCheck('product'),
  'product-placeholders': productCheck('product-placeholders'),
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
    'clippy-http3',
    'test-http3',
    'engine-docs',
    'docs',
  ],
  release: ['engine-package', 'msrv'],
  js: ['js-policy', 'protocol', 'js-check', 'js-lint', 'js-test', 'js-build'],
  repo: ['text', 'exceptions', 'tauri-pairs', 'product', 'commits'],
  'supply-chain': ['unused-deps', 'deny', 'npm-audit'],
  // Only when publishing a release, never in DEFAULT: the product identity still holds placeholders until the publisher's details are decided (D29). `product` first, so committed generated files are what is checked. Every release script runs this group before it builds anything.
  publish: ['product', 'product-placeholders'],
  // What the extension's release scripts run instead of `publish`: the extension alone ships the Public Suffix List, so only its releases wait on the list's age, which needs the registry and changes as the calendar moves (D35).
  'publish-extension': ['publish', 'suffix-list'],
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
  '--tool-specs': () => console.log(toolSpecs()),
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
    // Checks that need a package from `pnpm install` (the ESLint API, the product generator) are async and load it when they run: CI runs the Rust and supply-chain groups without installing them.
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
