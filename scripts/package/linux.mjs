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
