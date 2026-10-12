// Regenerates src/generated and src/index.ts from crates/protocol (`pnpm protocol:generate`).
//
// The types are written to a temporary folder first and replace src/generated only once the whole generation succeeded, so a build or test failure leaves the committed types in place. Replacing the folder as a whole means a type removed from crates/protocol does not leave its file behind (ts-rs only ever writes files). Every file is formatted with the repository's Prettier settings on the way, so the generated files pass the same format check as the rest of the package.
import { spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as prettier from 'prettier';
import { indexSource } from './write-index.mjs';

const PACKAGE = join(dirname(fileURLToPath(import.meta.url)), '..');
const GENERATED = join(PACKAGE, 'src', 'generated');
const INDEX = join(PACKAGE, 'src', 'index.ts');
const CRATE = 'wirecatch-protocol';

/** Runs the crate's export tests with ts-rs (and the constants test) writing into `folder`. */
function exportTypes(folder) {
  const args = ['test', '-p', CRATE, '--locked', '--quiet'];
  const result = spawnSync('cargo', args, {
    cwd: PACKAGE,
    stdio: 'inherit',
    // `.cargo/config.toml` sets the committed folder for plain `cargo test`; a variable already set wins over it.
    env: { ...process.env, TS_RS_EXPORT_DIR: folder },
  });
  if (result.error) throw new Error(`Could not start cargo: ${result.error.message}`);
  if (result.status !== 0) {
    throw new Error(
      `\`cargo ${args.join(' ')}\` failed with status ${String(result.status)}; src/generated is unchanged.`,
    );
  }
}

/** `text` formatted as Prettier formats the file at `path`, which decides the settings and the parser. */
async function formatted(text, path) {
  const options = await prettier.resolveConfig(path);
  return prettier.format(text, { ...options, filepath: path });
}

/** Every generated file, formatted, keyed by where it goes. */
async function generatedFiles(staging) {
  const files = new Map();
  for (const name of readdirSync(staging)) {
    const path = join(GENERATED, name);
    files.set(path, await formatted(readFileSync(join(staging, name), 'utf8'), path));
  }
  files.set(INDEX, await formatted(indexSource(staging), INDEX));
  return files;
}

const staging = mkdtempSync(join(tmpdir(), 'wirecatch-protocol-'));
try {
  exportTypes(staging);
  const files = await generatedFiles(staging);
  rmSync(GENERATED, { recursive: true, force: true });
  mkdirSync(GENERATED);
  for (const [path, text] of files) writeFileSync(path, text);
} catch (error) {
  console.error(error instanceof Error ? error.message : error);
  process.exitCode = 1;
} finally {
  rmSync(staging, { recursive: true, force: true });
}
