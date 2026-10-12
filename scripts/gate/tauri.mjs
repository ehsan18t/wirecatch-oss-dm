// Tauri's Rust crates and their JavaScript packages stay on the same major.minor (Tauri warns at build time when they differ).
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { cargoMetadata, GateError, ROOT } from './process.mjs';

/** Rust and JS packages that must stay on the same major.minor. */
const TAURI_PAIRS = [
  ['tauri', '@tauri-apps/api'],
  ['tauri-plugin-dialog', '@tauri-apps/plugin-dialog'],
];
const DESKTOP_APP = 'apps/desktop';

function majorMinor(version) {
  const match = /(\d+)\.(\d+)/.exec(version);
  return match ? `${match[1]}.${match[2]}` : version;
}

/** What is wrong with one pair, or null. */
function pairProblem([crate, npm], resolved) {
  const rust = resolved.get(crate) ?? [];
  if (rust.length !== 1) {
    return `Expected one resolved version of ${crate}, found ${rust.length > 0 ? rust.join(', ') : 'none'}; update TAURI_PAIRS in scripts/gate/tauri.mjs if it is no longer used.`;
  }
  const manifest = join(ROOT, DESKTOP_APP, 'node_modules', npm, 'package.json');
  if (!existsSync(manifest)) {
    return `${npm} is not installed in ${DESKTOP_APP}. Run \`pnpm install\` first, or update TAURI_PAIRS if it is no longer used.`;
  }
  const js = JSON.parse(readFileSync(manifest, 'utf8')).version;
  if (majorMinor(rust[0]) !== majorMinor(js)) {
    return `${crate} ${rust[0]} and ${npm} ${js} must be on the same major.minor; update them together.`;
  }
  return null;
}

/** Compares the versions actually resolved (Cargo.lock and the installed npm packages), so a lockfile-only update cannot drift unnoticed. */
function tauriPairs() {
  const resolved = new Map();
  for (const pkg of cargoMetadata({ deps: true }).packages) {
    if (pkg.source !== null)
      resolved.set(pkg.name, [...(resolved.get(pkg.name) ?? []), pkg.version]);
  }
  const problems = TAURI_PAIRS.map((pair) => pairProblem(pair, resolved)).filter(
    (problem) => problem !== null,
  );
  if (problems.length > 0) throw new GateError(problems.join('\n'));
}

export const TAURI_CHECKS = { 'tauri-pairs': tauriPairs };
