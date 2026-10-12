// Lints for Linux from Windows and macOS (`clippy-linux`).
import { capture, cargo, cargoMetadata, GateError } from './process.mjs';
import { BRIDGE_CRATE, TEST_FEATURES } from './rust.mjs';

/** The system Linux CI checks, linted from other systems by `clippy-linux`. */
const LINUX_TARGET = 'x86_64-unknown-linux-gnu';
/** A package in a crate's dependency tree that compiles C for the target (ring and bundled SQLite build with it), which needs a C compiler for Linux. */
const C_COMPILER_CRATE = /^cc v/m;

/**
 * The workspace crates that can be linted for Linux without a C compiler for Linux: those whose dependency tree for Linux, dev-dependencies included (`--all-targets`), never compiles C. The list grows by itself as crates stop depending on ring and other C code.
 */
function linuxLintableCrates() {
  return cargoMetadata()
    .packages.map((pkg) => pkg.name)
    .filter((name) => {
      const args = [
        'tree',
        '-p',
        name,
        '--target',
        LINUX_TARGET,
        '--edges',
        'normal,build,dev',
        '--prefix',
        'none',
        '--format',
        '{p}',
        '--locked',
      ];
      return !C_COMPILER_CRATE.test(capture('cargo', args).out);
    });
}

/**
 * Lints for Linux from Windows and macOS, so code behind `cfg(unix)` or `cfg(target_os = "linux")` is checked before Linux CI sees it. On Linux the `clippy` check already covers it. Crates that compile C for their target cannot be checked this way without a C cross-compiler, so only the crates that do not are (see linuxLintableCrates).
 */
export function clippyLinux() {
  if (process.platform === 'linux') {
    console.log('Skipped on Linux, where the clippy check already lints for Linux.');
    return;
  }
  const installed = capture('rustup', ['target', 'list', '--installed']).out.split(/\r?\n/);
  if (!installed.includes(LINUX_TARGET)) {
    throw new GateError(
      `The ${LINUX_TARGET} target is not installed, so code for Linux cannot be linted here. Add it with \`rustup target add ${LINUX_TARGET}\` (only its standard library; no C compiler is needed).`,
    );
  }
  const crates = linuxLintableCrates();
  if (crates.length === 0)
    throw new GateError(
      'No workspace crate can be linted for Linux from this system; every one compiles C.',
    );
  console.log(`Linting for ${LINUX_TARGET}: ${crates.join(', ')}.`);
  const packages = crates.flatMap((name) => ['-p', name]);
  const target = ['--target', LINUX_TARGET];
  const features = crates.includes(BRIDGE_CRATE) ? TEST_FEATURES : [];
  cargo(
    'clippy',
    ...target,
    ...packages,
    '--all-targets',
    '--locked',
    ...features,
    '--',
    '-D',
    'warnings',
  );
  if (crates.includes(BRIDGE_CRATE))
    cargo(
      'clippy',
      ...target,
      '-p',
      BRIDGE_CRATE,
      '--all-targets',
      '--locked',
      '--',
      '-D',
      'warnings',
    );
}
