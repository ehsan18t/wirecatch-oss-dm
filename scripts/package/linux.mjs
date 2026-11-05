import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';

/** What the app needs from the system on Linux: the libraries GPUI loads, by package name. */
const SYSTEM_LIBRARIES = {
  deb: [
    'libfontconfig1',
    'libfreetype6',
    'libxkbcommon0',
    'libxkbcommon-x11-0',
    'libwayland-client0',
    'libx11-xcb1',
    'libxcb1',
    'libvulkan1',
  ],
  rpm: [
    'fontconfig',
    'freetype',
    'libxkbcommon',
    'libxkbcommon-x11',
    'libwayland-client',
    'libX11-xcb',
    'libxcb',
    'vulkan-loader',
  ],
};

/**
 * Where the app finds the libraries an AppImage carries (D142). Not `LD_LIBRARY_PATH`: every program the app starts would inherit it.
 * `--disable-new-dtags`: a RUNPATH does not reach what a library loads, and libxkbcommon-x11 loads libxcb-xkb.
 */
export const BUNDLED_LIBRARY_SEARCH =
  '-C link-arg=-Wl,--disable-new-dtags -C link-arg=-Wl,-rpath,$ORIGIN/../lib';

/** This machine, as `libraryLicence` asks it: a package tool's output, or nothing when the tool is missing or does not know. */
const THIS_MACHINE = {
  query(command, args) {
    try {
      return execFileSync(command, args, {
        encoding: 'utf8',
        stdio: ['ignore', 'pipe', 'ignore'],
      });
    } catch {
      return undefined;
    }
  },
  read: (file) => readFileSync(file, 'utf8'),
};

function rpmLicenceFiles(name, machine) {
  const listed = machine.query('rpm', ['-qL', name]) ?? '';
  return listed.split('\n').filter((line) => line.startsWith('/'));
}

/**
 * The licence of the system package that installed the library at `file`, which the library's licence requires alongside it (D117),
 * or nothing when no package tool knows the file. `file` is the real file, not the link ldd names: that is what the package databases list.
 */
export function libraryLicence(file, machine = THIS_MACHINE) {
  // Lines read `package:architecture: /path`.
  const debian = machine.query('dpkg-query', ['-S', file]);
  if (debian) return machine.read(`/usr/share/doc/${debian.split(':')[0]}/copyright`);

  const rpm = machine.query('rpm', ['-qf', '--qf', '%{NAME}\n%{SOURCERPM}', file]);
  if (!rpm) return undefined;
  const [name, sourceRpm] = rpm.split('\n');
  // A subpackage may ship no licence file of its own: Fedora's libxkbcommon-x11 relies on libxkbcommon's.
  const source = /^(.+)-[^-]+-[^-]+\.src\.rpm$/.exec(sourceRpm)?.[1];
  const own = rpmLicenceFiles(name, machine);
  const files = own.length > 0 || !source ? own : rpmLicenceFiles(source, machine);
  return files.length > 0 ? files.map((path) => machine.read(path)).join('\n\n') : undefined;
}

/** The AppImage's entry point. */
export function appRun(app) {
  return `#!/bin/sh
HERE="$(dirname "$(readlink -f "$0")")"
exec "$HERE/usr/bin/${app}" "$@"
`;
}

/** Whether `machine` can make the Linux packages for `arch`: the AppImage carries the machine's own libraries, and `ldd` and the pinned tools run on it. */
export function canPackageLinux(arch, machine = process) {
  return machine.platform === 'linux' && machine.arch === arch;
}

/** The newest C library version named in the output of `readelf -V`, `undefined` when it names none. */
export function newestGlibc(versionInfo) {
  return [...versionInfo.matchAll(/\bGLIBC_(\d+(?:\.\d+)+)/g)]
    .map(([, version]) => version)
    .sort((a, b) => a.localeCompare(b, 'en', { numeric: true }))
    .at(-1);
}

/** Each package format's dependencies, with the C library the binaries need: nfpm works out none itself. */
export function linuxDepends(glibc) {
  return {
    deb: [`libc6 (>= ${glibc})`, ...SYSTEM_LIBRARIES.deb],
    rpm: [`glibc >= ${glibc}`, ...SYSTEM_LIBRARIES.rpm],
  };
}
