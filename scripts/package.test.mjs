import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { join } from 'node:path';
import { test } from 'node:test';
import { ROOT } from './gate/process.mjs';

test('packaging prepares Rust backports before building with Cargo', () => {
  // Exercise the real packaging entry point with a cold setup and stop at the build.
  const preload = `
import assert from 'node:assert/strict';
import childProcess from 'node:child_process';
import { writeSync } from 'node:fs';
import { syncBuiltinESMExports } from 'node:module';
import { basename } from 'node:path';

let prepared = false;
childProcess.execFileSync = (command, args) => {
  if (command === process.execPath) {
    const script = basename(args[0]);
    if (script === 'prepare-rust-backports.mjs') prepared = true;
    else assert.equal(script, 'gate.mjs');
    return;
  }
  assert.equal(command, 'cargo');
  assert.equal(args[0], 'build');
  assert.ok(prepared, 'Cargo reached the build before Rust backports were prepared');
  writeSync(1, 'Cargo received the build after preparation');
  process.exit(0);
};
syncBuiltinESMExports();
`;
  const result = spawnSync(
    process.execPath,
    [
      '--import',
      `data:text/javascript,${encodeURIComponent(preload)}`,
      join(ROOT, 'scripts', 'package.mjs'),
      '--target',
      'x86_64-pc-windows-msvc',
    ],
    { cwd: ROOT, encoding: 'utf8' },
  );
  assert.equal(result.status, 0, result.stderr ?? result.error?.message);
  assert.match(result.stdout, /Cargo received the build after preparation/);
});
