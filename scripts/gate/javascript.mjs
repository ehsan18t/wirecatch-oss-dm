// The JavaScript checks: the generated protocol types are current, every package is linted the one way (js-policy.mjs), and the packages check, lint, test, build and audit.
import { readdirSync, readFileSync } from 'node:fs';
import { join, relative } from 'node:path';
import { jsPolicy } from './js-policy.mjs';
import { GateError, pnpm, ROOT } from './process.mjs';

const GENERATED_TYPES = 'packages/protocol/src';

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

export const JS_CHECKS = {
  protocol: protocolDrift,
  'js-policy': jsPolicy,
  'js-check': () => pnpm('check'),
  'js-lint': () => pnpm('lint'),
  'js-test': () => pnpm('test'),
  'js-build': () => pnpm('build'),
  // Ignored advisories live in `auditConfig` in pnpm-workspace.yaml. GHSA-86w9-cpqp-85rv (node-forge, no fixed release) reaches us only through web-ext's Android device support, which the Firefox signing script never uses.
  'npm-audit': () => pnpm('audit'),
};
