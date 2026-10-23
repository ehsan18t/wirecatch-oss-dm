// The Rust checks: lint policy, clippy, tests, the published engine, and the cargo tools the supply-chain checks use.
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { capture, cargo, cargoMetadata, GateError, options, ROOT, run } from './process.mjs';

/** Features every workspace-wide cargo command needs. `test-overrides` exists for the bridge's end-to-end tests only and is never enabled in a build. */
export const TEST_FEATURES = ['--features', 'wirecatch-bridge/test-overrides'];

/** Cargo tools some checks run, at the versions both `--install-tools` (built from source) and CI (prebuilt, from `--tool-specs`) install. */
export const TOOLS = {
  'cargo-deny': '0.19.0',
  'cargo-machete': '0.9.2',
  'cargo-nextest': '0.9.148',
};

export const DESKTOP_CRATE = 'wirecatch-desktop';
export const BRIDGE_CRATE = 'wirecatch-bridge';
const ENGINE_CRATE = 'wirecatch-engine';
const TAURI_CONFIG = 'apps/desktop/src-tauri/tauri.conf.json';
/** Folders the published engine must not ship: its tests and examples use the unpublished test-support crate, so they cannot build from the package. */
const UNPACKAGED_ENGINE_FOLDERS = ['tests/', 'examples/'];

export const DENY_WARNINGS = ['--', '-D', 'warnings'];

function requireCargoTool(tool) {
  const subcommand = tool.replace(/^cargo-/, '');
  const probe = spawnSync('cargo', [subcommand, '--version'], { cwd: ROOT, encoding: 'utf8' });
  if (probe.status !== 0) {
    throw new GateError(
      `${tool} is not installed. Install the version CI uses with \`node scripts/gate.mjs --install-tools\`.`,
    );
  }
}

export function installTools() {
  for (const [tool, version] of Object.entries(TOOLS)) {
    cargo('install', tool, '--version', version, '--locked');
  }
}

/** The tools as `name@version`, comma separated: the `tool` input of the installer CI uses (taiki-e/install-action). */
export function toolSpecs(tools = TOOLS) {
  return Object.entries(tools)
    .map(([tool, version]) => `${tool}@${version}`)
    .join(',');
}

/** Tauri embeds the built frontend at compile time. When it has not been built, a placeholder page lets clippy and the tests compile, and says what to do if someone runs that build. */
function ensureDesktopFrontend() {
  const config = JSON.parse(readFileSync(join(ROOT, TAURI_CONFIG), 'utf8'));
  const folder = resolve(ROOT, dirname(TAURI_CONFIG), config.build.frontendDist);
  if (existsSync(folder)) return;
  console.log(
    `The desktop interface is not built; writing a placeholder to ${relative(ROOT, folder)} so the desktop crate compiles. Run \`pnpm --filter @wirecatch/desktop build\` for the real interface.`,
  );
  mkdirSync(folder, { recursive: true });
  writeFileSync(
    join(folder, 'index.html'),
    '<!doctype html><meta charset="utf-8"><title>Not built</title><p>The interface was not built. Run <code>pnpm --filter @wirecatch/desktop build</code>, then build the app again.</p>\n',
  );
}

/** Whether a manifest has `workspace = true` inside its `[lints]` table. */
function followsWorkspaceLints(manifest) {
  const table = /^\[lints\][^\S\r\n]*\r?\n([\s\S]*?)(?=^\[|(?![\s\S]))/m.exec(manifest);
  return table !== null && /^\s*workspace\s*=\s*true\s*(#.*)?$/m.test(table[1]);
}

/** Every crate opts into `[workspace.lints]`, so no crate can quietly follow a weaker policy. */
function lintPolicy() {
  const problems = [];
  for (const pkg of cargoMetadata().packages) {
    if (!followsWorkspaceLints(readFileSync(pkg.manifest_path, 'utf8'))) {
      const where = relative(ROOT, pkg.manifest_path);
      problems.push(
        `${where}: add \`[lints] workspace = true\`; every crate follows the workspace lint policy.`,
      );
    }
  }
  if (problems.length > 0) throw new GateError(problems.join('\n'));
}

function desktopExclusion({ desktop }) {
  return desktop ? [] : ['--exclude', DESKTOP_CRATE];
}

function clippy() {
  if (options.desktop) ensureDesktopFrontend();
  cargo(
    'clippy',
    '--workspace',
    ...desktopExclusion(options),
    '--all-targets',
    '--locked',
    ...TEST_FEATURES,
    ...DENY_WARNINGS,
  );
  // The bridge as it ships: without the test-only feature, code behind `cfg(not(feature = "test-overrides"))` is linted too.
  cargo('clippy', '-p', BRIDGE_CRATE, '--all-targets', '--locked', ...DENY_WARNINGS);
  // The desktop crate alone, its tests included, so a feature it needs but only another crate turns on cannot hide behind the workspace build.
  if (options.desktop)
    cargo('clippy', '-p', DESKTOP_CRATE, '--all-targets', '--locked', ...DENY_WARNINGS);
}

/** The arguments of the one test run: every crate, the desktop crate included, in one feature resolution, so the dependencies are built once. cargo-nextest runs each test in a process of its own and every test binary at once; `.config/nextest.toml` names the tests that must not overlap. */
export function testArguments({ desktop, failFast }) {
  return [
    'nextest',
    'run',
    '--workspace',
    ...desktopExclusion({ desktop }),
    '--locked',
    ...(failFast ? [] : ['--no-fail-fast']),
    ...TEST_FEATURES,
  ];
}

/** cargo-nextest does not run doctests, so cargo runs them after it, with the same crates and features. */
export function doctestArguments({ desktop, failFast }) {
  return [
    'test',
    '--doc',
    '--workspace',
    ...desktopExclusion({ desktop }),
    '--locked',
    ...(failFast ? [] : ['--no-fail-fast']),
    ...TEST_FEATURES,
  ];
}

/** Read by the tests that start the bridge (`BRIDGE_VARIABLE` in crates/test-support/src/bridge.rs). */
const BRIDGE_VARIABLE = 'WIRECATCH_TEST_BRIDGE';

/** The bridge the workspace test build makes, with `test-overrides` (TEST_FEATURES), and the bridge's own tests run. Every other test that starts the bridge takes this one: building their own would relink this file while other test processes run it. */
function workspaceBridge() {
  const metadata = cargoMetadata();
  const bin = metadata.packages
    .find((pkg) => pkg.name === BRIDGE_CRATE)
    ?.targets.find((target) => target.kind.includes('bin'));
  if (!bin) throw new GateError(`${BRIDGE_CRATE} has no executable target.`);
  const file = process.platform === 'win32' ? `${bin.name}.exe` : bin.name;
  return join(metadata.target_directory, 'debug', file);
}

/** Runs each cargo command; with `--no-fail-fast`, also the ones after a failure, so one failure cannot hide what the others would show. */
function cargoEach(commands, { env } = {}) {
  const failures = [];
  for (const args of commands) {
    try {
      run('cargo', args, { env });
    } catch (error) {
      if (options.failFast || !(error instanceof GateError)) throw error;
      failures.push(error.message);
    }
  }
  if (failures.length > 0) throw new GateError(failures.join('\n'));
}

function test() {
  requireCargoTool('cargo-nextest');
  if (options.desktop) ensureDesktopFrontend();
  cargoEach([testArguments(options), doctestArguments(options)], {
    env: { [BRIDGE_VARIABLE]: workspaceBridge() },
  });
}

/** The engine with its off-by-default http3 feature, which no other check compiles into a test. */
function testHttp3() {
  requireCargoTool('cargo-nextest');
  const engine = ['-p', ENGINE_CRATE, '--locked', '--features', 'http3'];
  cargoEach([
    ['nextest', 'run', ...engine],
    ['test', '--doc', ...engine],
  ]);
}

/** The published engine still builds with the oldest Rust its manifest promises. */
function msrv() {
  const engine = cargoMetadata().packages.find((p) => p.name === ENGINE_CRATE);
  const version = engine?.rust_version;
  if (!version)
    throw new GateError(
      `${ENGINE_CRATE} has no rust-version in its manifest; it is published and must state one.`,
    );
  run('rustup', ['toolchain', 'install', version, '--profile', 'minimal']);
  cargo(`+${version}`, 'check', '-p', ENGINE_CRATE, '--locked');
}

/** Builds the engine as published, then checks the package ships the library only (`include` in its Cargo.toml). */
function enginePackage() {
  cargo('package', '-p', ENGINE_CRATE, '--locked', '--allow-dirty');
  const files = capture('cargo', [
    'package',
    '-p',
    ENGINE_CRATE,
    '--list',
    '--locked',
    '--allow-dirty',
  ])
    .out.split(/\r?\n/)
    .map((line) => line.trim().replaceAll('\\', '/'))
    .filter(Boolean);
  const shipped = files.filter((file) =>
    UNPACKAGED_ENGINE_FOLDERS.some((folder) => file.startsWith(folder)),
  );
  if (shipped.length > 0) {
    throw new GateError(
      `The ${ENGINE_CRATE} package must ship the library only, but it lists:\n${shipped.join('\n')}\nKeep \`include\` in crates/engine/Cargo.toml to the library's files.`,
    );
  }
}

export const RUST_CHECKS = {
  fmt: () => cargo('fmt', '--all', '--check'),
  'lint-policy': lintPolicy,
  clippy,
  'clippy-http3': () =>
    cargo(
      'clippy',
      '-p',
      ENGINE_CRATE,
      '-p',
      'wirecatch-cli',
      '--all-targets',
      '--locked',
      '--features',
      'wirecatch-engine/http3,wirecatch-cli/http3',
      ...DENY_WARNINGS,
    ),
  test,
  'test-http3': testHttp3,
  // With every feature, as docs.rs builds them (`package.metadata.docs.rs` in the engine's Cargo.toml).
  'engine-docs': () =>
    run('cargo', ['doc', '-p', ENGINE_CRATE, '--no-deps', '--locked', '--all-features'], {
      env: { RUSTDOCFLAGS: '-D warnings' },
    }),
  // Every other library's documentation, so a broken or private link fails here as the engine's does. The desktop app is a program, not a library to document.
  docs: () =>
    run(
      'cargo',
      [
        'doc',
        '--workspace',
        '--lib',
        '--no-deps',
        '--locked',
        '--exclude',
        ENGINE_CRATE,
        '--exclude',
        DESKTOP_CRATE,
      ],
      { env: { RUSTDOCFLAGS: '-D warnings' } },
    ),
  // Verifies the package builds as published and ships the library only; git cleanliness is not its concern, so it also runs before a change is committed.
  'engine-package': enginePackage,
  msrv,
  deny: () => {
    requireCargoTool('cargo-deny');
    cargo('deny', '--locked', 'check');
  },
  'unused-deps': () => {
    requireCargoTool('cargo-machete');
    cargo('machete');
  },
};
