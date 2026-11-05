import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  appRun,
  BUNDLED_LIBRARY_SEARCH,
  canPackageLinux,
  libraryLicence,
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

/** A machine whose package tools answer from `answers`, keyed by the command line, and whose files are `files`. */
const machineWith = (answers, files = {}) => ({
  query: (command, args) => answers[[command, ...args].join(' ')],
  read: (file) => files[file],
});
const RPM_OWNER = 'rpm -qf --qf %{NAME}\n%{SOURCERPM} /lib/libx.so.1';

test('a library on a Debian system takes its package copyright file', () => {
  const machine = machineWith(
    { 'dpkg-query -S /lib/libx.so.1': 'libx1:amd64: /lib/libx.so.1\n' },
    { '/usr/share/doc/libx1/copyright': 'debian terms' },
  );
  assert.equal(libraryLicence('/lib/libx.so.1', machine), 'debian terms');
});

test('a library on an RPM system takes the licence files of its package', () => {
  const machine = machineWith(
    {
      [RPM_OWNER]: 'libx\nlibx-1.0-2.fc44.src.rpm',
      'rpm -qL libx': '/usr/share/licenses/libx/COPYING\n/usr/share/licenses/libx/NOTICE\n',
    },
    { '/usr/share/licenses/libx/COPYING': 'terms', '/usr/share/licenses/libx/NOTICE': 'notice' },
  );
  assert.equal(libraryLicence('/lib/libx.so.1', machine), 'terms\n\nnotice');
});

test('an RPM subpackage with no licence file takes the one of the package it was built with', () => {
  const machine = machineWith(
    {
      [RPM_OWNER]: 'libx-extra\nlibx-1.0-2.fc44.src.rpm',
      'rpm -qL libx-extra': '(contains no files)\n',
      'rpm -qL libx': '/usr/share/licenses/libx/COPYING\n',
    },
    { '/usr/share/licenses/libx/COPYING': 'terms' },
  );
  assert.equal(libraryLicence('/lib/libx.so.1', machine), 'terms');
});

test('a library no package tool can account for has no licence to give', () => {
  assert.equal(libraryLicence('/lib/libx.so.1', machineWith({})), undefined);
});
