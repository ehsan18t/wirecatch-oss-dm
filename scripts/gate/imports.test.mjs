import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const SCRIPTS = fileURLToPath(new URL('..', import.meta.url));

/** A static `import` or `export ... from` at the start of a line: its source is the first or second group. */
const STATIC_IMPORT = /^(?:import|export)\b[^;]+?\bfrom\s+'([^']+)'|^import\s+'([^']+)'/gm;

/** Every package the modules reachable from `entry` import statically, as `file: package`. Dynamic `import()` is left out: it runs only when a check does. */
function packagesLoadedAtStart(entry) {
  const seen = new Set();
  const packages = [];
  const visit = (file) => {
    if (seen.has(file)) return;
    seen.add(file);
    for (const [, from, bare] of readFileSync(file, 'utf8').matchAll(STATIC_IMPORT)) {
      const source = from ?? bare;
      if (source.startsWith('.')) visit(resolve(dirname(file), source));
      else if (!source.startsWith('node:')) packages.push(`${relative(SCRIPTS, file)}: ${source}`);
    }
  };
  visit(entry);
  return packages;
}

// CI runs the Rust and supply-chain groups without `pnpm install`, so the gate must start on Node's own modules alone.
test('the gate loads no package from pnpm install before a check runs', () => {
  assert.deepEqual(packagesLoadedAtStart(join(SCRIPTS, 'gate.mjs')), []);
});
