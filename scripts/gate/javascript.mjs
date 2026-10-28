// The JavaScript checks: the generated protocol types are current, every package is linted the one way (js-policy.mjs), and the packages check, lint, test, build and audit.
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { join, relative } from 'node:path';
import { jsPolicy } from './js-policy.mjs';
import { capture, GateError, pnpm, ROOT } from './process.mjs';

const GENERATED_TYPES = 'packages/protocol/src';
/** The generated types of each released protocol version, copied once when it was released (D32.8). */
const RELEASED_SNAPSHOTS = 'crates/protocol/released';
/** The branch whose snapshots are released: one it has can never change. */
const RELEASED_BASE = 'origin/main';

/** Every file under `dir`, with its contents, keyed by its path from the repository root. */
function folderContents(dir) {
  const contents = new Map();
  const walk = (folder) => {
    for (const entry of readdirSync(folder, { withFileTypes: true })) {
      const path = join(folder, entry.name);
      if (entry.isDirectory()) walk(path);
      else contents.set(relative(ROOT, path), readFileSync(path, 'utf8'));
    }
  };
  walk(join(ROOT, dir));
  return contents;
}

/** Regenerates the TypeScript types and fails if that changed any file: what was there did not match `crates/protocol`. It compares the files themselves, so it also works before a change is committed. */
function protocolDrift() {
  const before = folderContents(GENERATED_TYPES);
  pnpm('protocol:generate');
  const after = folderContents(GENERATED_TYPES);
  const paths = new Set([...before.keys(), ...after.keys()]);
  const changed = [...paths].filter((path) => before.get(path) !== after.get(path));
  if (changed.length > 0) {
    throw new GateError(
      `The generated TypeScript protocol types were out of date; they are regenerated now, so review and commit them:\n${changed.join('\n')}`,
    );
  }
}

/** The `paths` whose text in the working tree (`here`, null for a file that is gone) is not their text on the released branch (`there`). */
export function changedSnapshots(paths, there, here) {
  return paths.filter((path) => here(path) !== there(path));
}

/** A released protocol version is frozen (D32.8): fails when a snapshot of one that main already has was changed or removed, since peers built for it read exactly what it holds. */
function releasedSnapshotsKept() {
  const hint = `Fetch the main branch (\`git fetch origin main\`): the released protocol versions are read from ${RELEASED_BASE}.`;
  const paths = capture(
    'git',
    ['ls-tree', '-r', '--name-only', RELEASED_BASE, '--', RELEASED_SNAPSHOTS],
    { hint },
  )
    .out.split('\n')
    .filter(Boolean);
  const changed = changedSnapshots(
    paths,
    (path) => capture('git', ['show', `${RELEASED_BASE}:${path}`]).out,
    (path) => (existsSync(join(ROOT, path)) ? readFileSync(join(ROOT, path), 'utf8') : null),
  );
  if (changed.length > 0) {
    throw new GateError(
      `A released protocol version never changes (D32.8), but these snapshots differ from ${RELEASED_BASE}; restore them, and put a change in the version being built instead:\n${changed.join('\n')}`,
    );
  }
}

export const JS_CHECKS = {
  protocol: () => {
    protocolDrift();
    releasedSnapshotsKept();
  },
  'js-policy': jsPolicy,
  'js-check': () => pnpm('check'),
  'js-lint': () => pnpm('lint'),
  'js-test': () => pnpm('test'),
  'js-build': () => pnpm('build'),
  // Ignored advisories, each with its reason, live in `auditConfig` in pnpm-workspace.yaml.
  'npm-audit': () => pnpm('audit'),
};
