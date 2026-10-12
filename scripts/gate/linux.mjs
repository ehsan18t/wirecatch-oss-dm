// Lints for Linux from Windows and macOS (`clippy-linux`).
import { capture, cargo, cargoMetadata, GateError } from './process.mjs';
import { BRIDGE_CRATE, TEST_FEATURES } from './rust.mjs';

/** The system Linux CI checks, linted from other systems by `clippy-linux`. Its standard library comes with the toolchain (`targets` in rust-toolchain.toml). */
const LINUX_TARGET = 'x86_64-unknown-linux-gnu';
/** Build-time crates that compile native code for the target or look for a native library on it (ring and bundled SQLite use `cc`); a crate whose dependencies use one cannot be checked for Linux without a Linux C toolchain and libraries. */
const NATIVE_BUILD_CRATES = ['cc', 'cmake', 'pkg-config', 'system-deps'];
/** What `cargo tree --invert` says when the crate is not in the graph at all. */
const NOT_IN_GRAPH = /did not match any packages/;
const DENY_WARNINGS = ['--', '-D', 'warnings'];

/** The features the lint run turns on for `crates`, so the selection below resolves exactly what it will build. */
function featuresFor(crates) {
  return crates.includes(BRIDGE_CRATE) ? TEST_FEATURES : [];
}

/** For `crates` built together for Linux, as the lint run builds them: each workspace crate that reaches a native build crate, with the native crates it reaches. */
function nativeUsers(crates, members) {
  const users = new Map();
  const packages = crates.flatMap((name) => ['-p', name]);
  for (const native of NATIVE_BUILD_CRATES) {
    const args = [
      'tree',
      ...packages,
      ...featuresFor(crates),
      '--target',
      LINUX_TARGET,
      '--edges',
      'normal,build,dev',
      '--invert',
      native,
      '--prefix',
      'none',
      '--format',
      '{p}',
      '--locked',
    ];
    const result = capture('cargo', args, { allowed: [0, 101] });
    if (result.status !== 0) {
      if (NOT_IN_GRAPH.test(result.err)) continue;
      throw new GateError(`\`cargo ${args.join(' ')}\` failed:\n${result.err.trim()}`);
    }
    for (const name of result.out.split('\n').map((line) => line.split(' ')[0])) {
      if (members.includes(name))
        users.set(name, [...new Set([...(users.get(name) ?? []), native])]);
    }
  }
  return users;
}

/**
 * Splits the workspace into the crates that can be linted for Linux from here and those that cannot, with the reason. A crate is out when its own dependencies reach a native build crate; the rest are then checked together with the features the lint run uses, since unified features can pull a native crate into a crate that is clean alone, and any crate that does is dropped too. The list grows by itself as crates stop depending on ring and other native code.
 */
function linuxSelection() {
  const members = cargoMetadata().packages.map((pkg) => pkg.name);
  const skipped = new Map();
  let lintable = members.filter((name) => {
    const native = nativeUsers([name], members).get(name);
    if (native) skipped.set(name, native);
    return !native;
  });
  for (
    let users = nativeUsers(lintable, members);
    users.size > 0;
    users = nativeUsers(lintable, members)
  ) {
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
