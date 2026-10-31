#!/usr/bin/env node
// The quality gate: every check a change must pass, in one place. CI, CLAUDE.md, CONTRIBUTING.md and scripts/linux-test.sh run this script instead of repeating its commands, so they cannot drift apart. This file is the runner; the checks live in scripts/gate/, one module per subject.
//
//   node scripts/gate.mjs                    every check CI runs (see DEFAULT)
//   node scripts/gate.mjs rust js            named checks or groups (see GROUPS)
//   node scripts/gate.mjs publish            what a release needs on top of DEFAULT
//   node scripts/gate.mjs publish-extension  `publish` plus what a release of the browser extension needs
//   node scripts/gate.mjs release-build      what a release build runs: `publish`, with placeholders only warned about
//   node scripts/gate.mjs commit-msg FILE    check one commit message (the commit-msg hook)
//   node scripts/gate.mjs --install-tools    install the cargo tools some checks need, at the versions in TOOLS (scripts/gate/rust.mjs)
//   node scripts/gate.mjs --tool-specs       print those tools as name@version, for CI's prebuilt installer
//   node scripts/gate.mjs --list             list checks and groups
//
// Options, before or after the names:
//   --no-desktop    leave out the desktop app's crate: for machines without the system libraries GPUI links against (it says so when it does)
//   --no-fail-fast  run every Rust test binary even after one fails
//   --skip=CHECK    leave out one check the named groups hold (repeatable), as CI's Windows JavaScript job leaves out `protocol`, which Linux runs
//
// The commit range for `commits` is GATE_COMMIT_RANGE when set, otherwise `origin/main..HEAD`. Written for Node 18 and later, so it also runs where only an older Node is installed (scripts/linux-test.sh).

import { COMMIT_CHECKS, commitMessageFile } from './gate/commits.mjs';
import { EXCEPTION_CHECKS } from './gate/exceptions.mjs';
import { BROWSER_CHECKS } from './gate/browser.mjs';
import { JS_CHECKS } from './gate/javascript.mjs';
import { clippyLinux } from './gate/linux.mjs';
import { GateError, options } from './gate/process.mjs';
import { DESKTOP_CRATE, installTools, RUST_CHECKS, toolSpecs } from './gate/rust.mjs';
import { SUFFIX_LIST_CHECKS } from './gate/suffix-list.mjs';
import { TEST_TIMING_CHECKS } from './gate/test-timing.mjs';
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
  ...TEST_TIMING_CHECKS,
  'clippy-linux': clippyLinux,
  ...JS_CHECKS,
  ...BROWSER_CHECKS,
  ...TEXT_CHECKS,
  ...COMMIT_CHECKS,
  ...SUFFIX_LIST_CHECKS,
  product: productCheck('product'),
  'product-placeholders': productCheck('product-placeholders'),
  'product-placeholders-warn': productCheck('product-placeholders-warn'),
};

const GROUPS = {
  // The http3 checks run here, on every system CI tests, because QUIC sockets behave differently on Windows. CI runs the two halves as separate jobs, side by side.
  rust: ['rust-lint', 'rust-test'],
  'rust-lint': [
    'fmt',
    'lint-policy',
    'cfg-allow',
    'test-timing',
    'clippy',
    'clippy-linux',
    'clippy-http3',
    'engine-docs',
    'docs',
  ],
  'rust-test': ['test', 'test-http3'],
  release: ['engine-package', 'msrv'],
  js: ['js-policy', 'protocol', 'js-check', 'js-lint', 'js-test', 'js-build'],
  // Apart from `js` so CI runs them alongside it: they take minutes and need no Rust. They skip themselves, saying so, only where their browser is not installed (WIRECATCH_REQUIRE_BROWSER=1 makes that fail too).
  browser: ['e2e-extension'],
  repo: ['text', 'exceptions', 'product', 'commits'],
  'supply-chain': ['unused-deps', 'deny', 'npm-audit'],
  // Only when publishing a release, never in DEFAULT: the product identity still holds placeholders until the publisher's details are decided (D29). `product` first, so committed generated files are what is checked. Every step that publishes runs this group first.
  publish: ['product', 'product-placeholders'],
  // What the extension's publishing step runs instead of `publish`: the extension alone ships the Public Suffix List, so only its releases wait on the list's age, which needs the registry and changes as the calendar moves (D35).
  'publish-extension': ['publish', 'suffix-list'],
  // What a release build runs before it builds: the same checks, but a placeholder is only warned about, so installers and store packages can be built and tried before the publisher's details are decided; publishing still refuses them (D49).
  'release-build': ['product', 'product-placeholders-warn'],
  'release-build-extension': ['release-build', 'suffix-list'],
};

/** What `node scripts/gate.mjs` runs with no names: every check CI runs. */
const DEFAULT = ['repo', 'rust', 'release', 'js', 'browser', 'supply-chain'];

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
  const skipped = new Set();
  for (const arg of rawArgs) {
    if (arg === '--no-desktop') options.desktop = false;
    else if (arg === '--no-fail-fast') options.failFast = false;
    else if (arg.startsWith('--skip=')) skipped.add(arg.slice('--skip='.length));
    else names.push(arg);
  }
  for (const name of skipped) {
    if (!CHECKS[name])
      throw new GateError(`Unknown check "${name}" in --skip. Run with --list to see them.`);
  }
  if (!options.desktop)
    console.log(
      `Leaving out ${DESKTOP_CRATE} (--no-desktop). This run does not cover the desktop app.`,
    );
  const command = COMMANDS[names[0]];
  if (command) {
    command(names[1]);
    return;
  }
  const checks = [...new Set(expand(names.length > 0 ? names : DEFAULT))].filter(
    (name) => !skipped.has(name),
  );
  if (skipped.size > 0) console.log(`Leaving out ${[...skipped].join(', ')} (--skip).`);
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
