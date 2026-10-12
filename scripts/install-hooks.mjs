#!/usr/bin/env node
// Points git at the repository's hooks (.githooks), so commit messages are checked when they are written. Runs from `pnpm install` (`prepare`). It changes nothing, and says why, outside a git checkout, in CI, or when another hooks folder is already configured.

import { spawnSync } from 'node:child_process';

const HOOKS = '.githooks';

function git(...args) {
  return spawnSync('git', args, { encoding: 'utf8' });
}

function main() {
  if (process.env.CI) return;
  if (git('rev-parse', '--git-dir').status !== 0) {
    console.log('Not a git checkout: the commit message hook is not installed.');
    return;
  }
  const current = git('config', '--get', 'core.hooksPath').stdout.trim();
  if (current === HOOKS) return;
  if (current) {
    console.log(
      `git already uses hooks from ${current}; leaving it. To check commit messages, call \`node scripts/gate.mjs commit-msg "$1"\` from your commit-msg hook.`,
    );
    return;
  }
  if (git('config', 'core.hooksPath', HOOKS).status === 0) {
    console.log(`Commit messages are now checked by ${HOOKS}/commit-msg.`);
  }
}

main();
