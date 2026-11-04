import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  appRun,
  BUNDLED_LIBRARY_SEARCH,
  canPackageLinux,
  linuxDepends,
  newestGlibc,
} from './linux.mjs';

test('the entry point leaves the library path to the programs the app starts', () => {
  const script = appRun('wirecatch-app');
  assert.doesNotMatch(script, /LD_LIBRARY_PATH/);
  assert.match(script, /exec "\$HERE\/usr\/bin\/wirecatch-app" "\$@"/);
});

test('the app finds the bundled libraries from its own file, down the load chain', () => {
  assert.match(BUNDLED_LIBRARY_SEARCH, /-rpath,\$ORIGIN\/\.\.\/lib/);
  assert.match(BUNDLED_LIBRARY_SEARCH, /--disable-new-dtags/);
});

test('linux packages are made only on a linux machine of the same architecture', () => {
  assert.equal(canPackageLinux('arm64', { platform: 'linux', arch: 'arm64' }), true);
  assert.equal(canPackageLinux('arm64', { platform: 'linux', arch: 'x64' }), false);
  assert.equal(canPackageLinux('x64', { platform: 'win32', arch: 'x64' }), false);
});

test('the newest C library version wins by number, not by text', () => {
  const versionInfo = [
    '  0x0010:   Name: GLIBC_2.2.5  Flags: none  Version: 3',
    '  0x0020:   Name: GLIBC_2.9  Flags: none  Version: 4',
    '  0x0030:   Name: GLIBC_2.34  Flags: none  Version: 5',
    '  0x0040:   Name: GCC_4.2.0  Flags: none  Version: 6',
    '  0x0050:   Name: GLIBC_PRIVATE  Flags: none  Version: 7',
    '  0x0060:   Name: GLIBC_ABI_DT_RELR  Flags: none  Version: 8',
  ].join('\n');
  assert.equal(newestGlibc(versionInfo), '2.34');
});

test('output that names no C library version gives none', () => {
  assert.equal(newestGlibc('No version information found in this file.'), undefined);
});

test('both package formats state the C library version in their own form', () => {
  const { deb, rpm } = linuxDepends('2.39');
  assert.equal(deb[0], 'libc6 (>= 2.39)');
  assert.equal(rpm[0], 'glibc >= 2.39');
  assert.ok(deb.includes('libxkbcommon0'));
  assert.ok(rpm.includes('libxkbcommon'));
});
