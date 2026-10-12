// Lints for Linux from Windows and macOS (`clippy-linux`).
import { capture, cargo, cargoMetadata, GateError } from './process.mjs';
import { BRIDGE_CRATE, DENY_WARNINGS, TEST_FEATURES } from './rust.mjs';

/** The system Linux CI checks, linted from other systems by `clippy-linux`. Its standard library comes with the toolchain (`targets` in rust-toolchain.toml). */
const LINUX_TARGET = 'x86_64-unknown-linux-gnu';
/** Build-time crates that compile native code for the target or look for a native library on it (ring and bundled SQLite use `cc`); a crate whose dependencies use one cannot be checked for Linux without a Linux C toolchain and libraries. */
const NATIVE_BUILD_CRATES = ['cc', 'cmake', 'pkg-config', 'system-deps'];
/** The features the lint run turns on for `crates`, so the selection below resolves exactly what it will build. */
function featuresFor(crates) {
  return crates.includes(BRIDGE_CRATE) ? TEST_FEATURES : [];
}

/** `cargo tree` for `crates` built together for Linux, with the lint run's features, one package per line (`name vX.Y.Z`). `extra` narrows it, as `--invert` does. */
function treeOf(crates, extra = []) {
  const args = [
    'tree',
    ...crates.flatMap((name) => ['-p', name]),
    ...featuresFor(crates),
    '--target',
    LINUX_TARGET,
    '--edges',
    'normal,build,dev',
    ...extra,
    '--prefix',
    'none',
    '--format',
    '{p}',
    '--locked',
  ];
  return capture('cargo', args)
    .out.split('\n')
    .map((line) => line.split(' '))
    .filter(([name]) => name);
}

/** The native build crates in a tree, by exact version, so an inverted tree is never ambiguous between two versions. */
function nativeCrates(tree) {
  return [
    ...new Set(
      tree
        .filter(([name]) => NATIVE_BUILD_CRATES.includes(name))
        .map(([name, version]) => `${name}@${version.replace(/^v/, '')}`),
    ),
  ];
}

/** Which of `crates`, built together, reach a native crate in their tree, with the native crates they reach. Only members of `crates` are returned, so the loop in linuxSelection removes at least one crate each round and ends. */
function nativeUsers(crates) {
  const users = new Map();
  for (const native of nativeCrates(treeOf(crates))) {
    for (const [name] of treeOf(crates, ['--invert', native])) {
      if (crates.includes(name))
        users.set(name, [...new Set([...(users.get(name) ?? []), native.split('@')[0]])]);
    }
  }
  return users;
}

/**
 * Splits the workspace into the crates that can be linted for Linux from here and those that cannot, with the reason. A crate is out when its own tree reaches a native build crate; the rest are then built together with the lint run's features, since unified features can pull a native crate into a crate that is clean alone, and any crate that then reaches one is dropped too. The list grows by itself as crates stop depending on ring and other native code.
 */
function linuxSelection() {
  const members = cargoMetadata().packages.map((pkg) => pkg.name);
  const skipped = new Map();
  let lintable = members.filter((name) => {
    const native = nativeCrates(treeOf([name])).map((crate) => crate.split('@')[0]);
    if (native.length > 0) skipped.set(name, [...new Set(native)]);
    return native.length === 0;
  });
  for (let users = nativeUsers(lintable); users.size > 0; users = nativeUsers(lintable)) {
    for (const [name, native] of users) skipped.set(name, native);
    lintable = lintable.filter((name) => !users.has(name));
  }
  return { lintable, skipped };
}

/**
 * Lints for Linux from Windows and macOS, so code behind `cfg(unix)` or `cfg(target_os = "linux")` is checked before Linux CI sees it. On Linux the `clippy` check already covers it. Crates whose dependencies build native code for the target need a Linux C toolchain, so they are linted for Linux only by the clippy check on Linux CI; this check says which.
 */
export function clippyLinux() {
  if (process.platform === 'linux') {
    console.log('Skipped on Linux, where the clippy check already lints for Linux.');
    return;
  }
  const installed = capture('rustup', ['target', 'list', '--installed']).out.split(/\r?\n/);
  if (!installed.includes(LINUX_TARGET)) {
    throw new GateError(
      `The ${LINUX_TARGET} target is not installed, so code for Linux cannot be linted here. rust-toolchain.toml lists it; add it with \`rustup target add ${LINUX_TARGET}\` (only its standard library; no C compiler is needed).`,
    );
  }
  const { lintable, skipped } = linuxSelection();
  const reasons = [...skipped].map(([name, native]) => `${name} (uses ${native.join(', ')})`);
  if (reasons.length > 0)
    console.log(
      `Left to Linux CI, since their dependencies build native code for Linux: ${reasons.join(', ')}.`,
    );
  if (lintable.length === 0)
    throw new GateError(
      'No workspace crate can be linted for Linux from this system; every one builds native code.',
    );
  console.log(`Linting for ${LINUX_TARGET}: ${lintable.join(', ')}.`);
  const target = ['--target', LINUX_TARGET];
  cargo(
    'clippy',
    ...target,
    ...lintable.flatMap((name) => ['-p', name]),
    '--all-targets',
    '--locked',
    ...featuresFor(lintable),
    ...DENY_WARNINGS,
  );
  // The bridge as it ships, as the clippy check also lints it.
  if (lintable.includes(BRIDGE_CRATE))
    cargo('clippy', ...target, '-p', BRIDGE_CRATE, '--all-targets', '--locked', ...DENY_WARNINGS);
}
