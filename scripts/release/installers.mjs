import { extname } from 'node:path';

const ARCHES = ['x64', 'arm64'];

export const INSTALLER_SUFFIXES = [
  ['windows', 'setup', '-setup.exe'],
  ['linux', 'appimage', '.AppImage'],
  ['linux', 'deb', '.deb'],
  ['linux', 'rpm', '.rpm'],
].flatMap(([system, kind, ending]) =>
  ARCHES.map((arch) => ({ system, arch, kind, suffix: `${system}-${arch}${ending}` })),
);

const INSTALLER_EXTENSIONS = new Set(
  INSTALLER_SUFFIXES.map(({ suffix }) => extname(suffix).toLowerCase()),
);

export function installerNames(product, version) {
  return INSTALLER_SUFFIXES.map(({ suffix, ...target }) => ({
    ...target,
    name: `${product}-${version}-${suffix}`,
  }));
}

export function installerName(product, arch, kind) {
  return installerNames(product.name, product.version).find(
    (installer) => installer.arch === arch && installer.kind === kind,
  ).name;
}

export function isInstaller(name) {
  return INSTALLER_EXTENSIONS.has(extname(name).toLowerCase());
}
