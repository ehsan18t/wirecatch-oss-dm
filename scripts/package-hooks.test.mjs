import assert from 'node:assert/strict';
import { execFileSync, spawn } from 'node:child_process';
import { once } from 'node:events';
import { copyFileSync, mkdtempSync, readFileSync, realpathSync, renameSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { fillTemplate } from './product/manifests.mjs';

const TEMPLATE = join(
  dirname(fileURLToPath(import.meta.url)),
  '..',
  'config/templates/postinst.sh',
);
const STAYS_UP = 'echo ready; read line || exit 0';
const IGNORES_SIGTERM = `trap '' TERM; ${STAYS_UP}`;

/** A process whose executable is `file`, running `script` until its standard input closes. */
async function startCopy(file, script) {
  const child = spawn(file, ['-c', script], { stdio: ['pipe', 'pipe', 'ignore'] });
  await once(child.stdout, 'data');
  return child;
}

/** As a package manager upgrades: the new file is renamed over the old one. */
function replaceProgram(app) {
  copyFileSync('/bin/sh', `${app}.new`);
  renameSync(`${app}.new`, app);
}

/** Runs the hook on the app at `app`, with `PATIENCE` cut to a second. */
function runHook(app) {
  const hook = fillTemplate(
    readFileSync(TEMPLATE, 'utf8'),
    { product: { name: 'App' }, executables: { app: 'app-under-test' } },
    'postinst.sh',
  )
    .replaceAll('/usr/bin/', () => `${dirname(app)}/`)
    .replace(/^PATIENCE=\d+$/m, 'PATIENCE=1');
  execFileSync('sh', ['-c', hook]);
}

const LINUX_ONLY = { skip: process.platform !== 'linux' };

/** Runs `body` with the path of a stand-in app: a copy of the shell, which stays up with no second program. */
async function withApp(body) {
  const folder = realpathSync(mkdtempSync(join(tmpdir(), 'wirecatch-hook-')));
  const app = join(folder, 'app-under-test');
  const copies = [];
  try {
    copyFileSync('/bin/sh', app);
    await body(app, async (script) => {
      const copy = await startCopy(app, script);
      copies.push(copy);
      return copy;
    });
  } finally {
    for (const copy of copies) copy.kill('SIGKILL');
    rmSync(folder, { recursive: true, force: true });
  }
}

test(
  'the post-install hook stops the copy whose program was replaced, and only that one',
  LINUX_ONLY,
  () =>
    withApp(async (app, start) => {
      const outdated = await start(STAYS_UP);
      replaceProgram(app);
      const current = await start(STAYS_UP);

      runHook(app);

      const [, outdatedSignal] = await once(outdated, 'exit');
      assert.equal(outdatedSignal, 'SIGTERM');
      // The hook has finished, so a signal meant for this copy has already been sent.
      current.stdin.end();
      const [currentCode, currentSignal] = await once(current, 'exit');
      assert.deepEqual([currentCode, currentSignal], [0, null]);
    }),
);

test('the post-install hook forces out a replaced copy that ignores SIGTERM', LINUX_ONLY, () =>
  withApp(async (app, start) => {
    const stuck = await start(IGNORES_SIGTERM);
    replaceProgram(app);

    runHook(app);

    const [, signal] = await once(stuck, 'exit');
    assert.equal(signal, 'SIGKILL');
  }),
);
