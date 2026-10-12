import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { EM_DASH_LITERAL } from './text.mjs';

/** A shell with no locale set, as CI runners and containers often have. */
const NO_LOCALE = { ...process.env, LANG: '', LC_ALL: '', LC_CTYPE: '' };

test('the em dash pattern works in a shell with no locale', () => {
  const dir = mkdtempSync(join(tmpdir(), 'text-check-'));
  try {
    writeFileSync(join(dir, 'dash.txt'), `a ${String.fromCodePoint(0x2014)} b\n`);
    writeFileSync(join(dir, 'clean.txt'), 'a, b\n');
    const result = spawnSync('git', ['grep', '--no-index', '-l', '-P', EM_DASH_LITERAL], {
      cwd: dir,
      env: NO_LOCALE,
      encoding: 'utf8',
    });
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.stdout.trim(), 'dash.txt');
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
