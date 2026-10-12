// The Rust checks: lint policy, clippy, tests, the published engine, and the cargo tools the supply-chain checks use.
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { capture, cargo, cargoMetadata, GateError, options, ROOT, run } from './process.mjs';

/** Features every workspace-wide cargo command needs. `test-overrides` exists for the bridge's end-to-end tests only and is never enabled in a build. */
export const TEST_FEATURES = ['--features', 'wirecatch-bridge/test-overrides'];

/** Cargo tools some checks run, with the versions CI installs (`--install-tools`). */
const TOOLS = {
  'cargo-deny': '0.19.0',
  'cargo-machete': '0.9.2',
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

function clippy() {
  if (options.desktop) ensureDesktopFrontend();
  const exclusion = options.desktop ? [] : ['--exclude', DESKTOP_CRATE];
  cargo(
    'clippy',
    '--workspace',
    ...exclusion,
    '--all-targets',
    '--locked',
    ...TEST_FEATURES,
    ...DENY_WARNINGS,
  );
  // The bridge as it ships: without the test-only feature, code behind `cfg(not(feature = "test-overrides"))` is linted too.
  cargo('clippy', '-p', BRIDGE_CRATE, '--all-targets', '--locked', ...DENY_WARNINGS);
}

function failFast() {
  return options.failFast ? [] : ['--no-fail-fast'];
}

function test() {
  cargo(
    'test',
    '--workspace',
    '--exclude',
    DESKTOP_CRATE,
    '--locked',
    ...failFast(),
    ...TEST_FEATURES,
  );
}

/** The desktop crate is tested on its own: its Tauri build script adds link paths that break other crates' doctests when everything links together. */
function testDesktop() {
  if (!options.desktop) return;
  ensureDesktopFrontend();
  cargo('test', '-p', DESKTOP_CRATE, '--locked', ...failFast());
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
  // The engine with its off-by-default http3 feature, which no other check compiles into a test.
  'test-http3': () => cargo('test', '-p', ENGINE_CRATE, '--locked', '--features', 'http3'),
  'test-desktop': testDesktop,
  // With every feature, as docs.rs builds them (`package.metadata.docs.rs` in the engine's Cargo.toml).
  'engine-docs': () =>
    run('cargo', ['doc', '-p', ENGINE_CRATE, '--no-deps', '--locked', '--all-features'], {
      env: { RUSTDOCFLAGS: '-D warnings' },
    }),
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
