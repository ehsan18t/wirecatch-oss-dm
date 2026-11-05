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

/** A process whose executable is `file`, running until its standard input closes. */
async function startCopy(file) {
  const child = spawn(file, ['-c', 'read line || exit 0'], { stdio: ['pipe', 'ignore', 'ignore'] });
  await once(child, 'spawn');
  return child;
}

test(
  'the post-install hook stops the copy whose program was replaced, and only that one',
  { skip: process.platform !== 'linux' },
  async () => {
    const folder = realpathSync(mkdtempSync(join(tmpdir(), 'wirecatch-hook-')));
    const app = join(folder, 'app-under-test');
    const copies = [];
    try {
      // A copy of the shell stands in for the app: it stays up without help from a second program.
      copyFileSync('/bin/sh', app);
      const outdated = await startCopy(app);
      copies.push(outdated);
      // As a package manager upgrades: the new file is renamed over the old one.
      copyFileSync('/bin/sh', `${app}.new`);
      renameSync(`${app}.new`, app);
      const current = await startCopy(app);
      copies.push(current);

      const hook = fillTemplate(
        readFileSync(TEMPLATE, 'utf8'),
        { product: { name: 'App' }, executables: { app: 'app-under-test' } },
        'postinst.sh',
      ).replaceAll('/usr/bin/', () => `${folder}/`);
      execFileSync('sh', ['-c', hook]);

      const [, outdatedSignal] = await once(outdated, 'exit');
      assert.equal(outdatedSignal, 'SIGTERM');
      // The hook has finished, so a signal meant for this copy has already been sent.
      current.stdin.end();
      const [currentCode, currentSignal] = await once(current, 'exit');
      assert.deepEqual([currentCode, currentSignal], [0, null]);
    } finally {
      for (const copy of copies) copy.kill('SIGKILL');
      rmSync(folder, { recursive: true, force: true });
    }
  },
);
