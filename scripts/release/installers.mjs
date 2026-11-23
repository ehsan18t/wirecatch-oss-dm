import { extname } from 'node:path';

const ARCHES = ['X64', 'Arm64'];

export const INSTALLER_SUFFIXES = [
  ['Windows', 'Setup', '-setup.exe'],
  ['Linux', 'AppImage', '.AppImage'],
  ['Linux', 'Deb', '.deb'],
  ['Linux', 'Rpm', '.rpm'],
].flatMap(([system, kind, ending]) =>
  ARCHES.map((arch) => ({
    system,
    arch,
    kind,
    suffix: `${system.toLowerCase()}-${arch.toLowerCase()}${ending}`,
  })),
);

const INSTALLER_EXTENSIONS = new Set(
  INSTALLER_SUFFIXES.map(({ suffix }) => extname(suffix).toLowerCase()),
);

/** The published installer prefix is independent of the display name. */
export function installerPrefix({ slug }) {
  return slug[0].toUpperCase() + slug.slice(1);
}

/** The filename pattern that the product generator also uses for the Rust updater. */
export function installerFilename(prefix, version, suffix) {
  return `${prefix}-${version}-${suffix}`;
}

export function installerNames(product) {
  const prefix = installerPrefix(product);
  return INSTALLER_SUFFIXES.map(({ system, arch, kind, suffix }) => ({
    system: system.toLowerCase(),
    arch: arch.toLowerCase(),
    kind: kind.toLowerCase(),
    name: installerFilename(prefix, product.version, suffix),
  }));
}

export function installerName(product, arch, kind) {
  return installerNames(product).find(
    (installer) => installer.arch === arch && installer.kind === kind,
  ).name;
}

export function isInstaller(name) {
  return INSTALLER_EXTENSIONS.has(extname(name).toLowerCase());
}
