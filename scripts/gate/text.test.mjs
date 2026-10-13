import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { CREDENTIAL_PATTERN, EM_DASH_LITERAL } from './text.mjs';

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

test('the credential pattern finds each provider shape and leaves split samples alone', () => {
  const dir = mkdtempSync(join(tmpdir(), 'text-check-'));
  // Each sample is put together here, so this file holds none whole.
  const samples = [
    ['sk', '_test_', '4eC39HqLyjWDarjtT1zdp7dc'],
    ['sk', '-ant-', 'api03-AbCdEfGhIjKlMnOp'],
    ['gh', 'p_', 'a1B2c3D4e5F6g7H8i9J0k1L2m3N4o5P6q7R8'],
    ['github', '_pat_', '11ABCDEFG0123456789_abc'],
    ['gl', 'pat-', 'AbCdEfGhIjKlMnOpQrSt'],
    ['np', 'm_', 'a1B2c3D4e5F6g7H8i9J0'],
    ['xo', 'xb-', '1234567890-abcdef'],
    ['AK', 'IA', 'IOSFODNN7EXAMPLE'],
    ['AI', 'za', 'SyA1b2C3d4E5f6G7h8I9j0k1'],
    ['ey', 'JhbGciOiJIUzI1NiJ9.eyJzdWIiOjF9', '.abc123xyz'],
    ['-----BEGIN RSA ', 'PRIVATE KEY', '-----'],
  ];
  try {
    samples.forEach((parts, i) =>
      writeFileSync(join(dir, `${String(i)}.txt`), `x ${parts.join('')} y\n`),
    );
    writeFileSync(join(dir, 'split.txt'), `x ${samples[7].join('{+}')} y\nAKIA is a prefix\n`);
    const result = spawnSync('git', ['grep', '--no-index', '-l', '-P', CREDENTIAL_PATTERN], {
      cwd: dir,
      env: NO_LOCALE,
      encoding: 'utf8',
    });
    assert.equal(result.status, 0, result.stderr);
    const found = result.stdout.trim().split('\n').sort();
    const expected = samples.map((_, i) => `${String(i)}.txt`).sort();
    assert.deepEqual(found, expected);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
