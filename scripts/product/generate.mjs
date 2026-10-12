// Writes every file generated from config/product.toml (`pnpm product:generate`): the list is `outputs` in outputs.mjs. A file that already has the right contents is left alone, so its timestamp does not change and nothing rebuilds for it.
import { mkdirSync, readFileSync, writeFileSync, existsSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { expectedOutputs } from './outputs.mjs';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..', '..');

/** Writes each expected file whose contents differ, and returns the paths it wrote. */
export async function generate(root) {
  const { expected } = await expectedOutputs(root);
  const written = [];
  for (const { path, text } of expected) {
    const file = join(root, path);
    if (existsSync(file) && readFileSync(file, 'utf8') === text) continue;
    mkdirSync(dirname(file), { recursive: true });
    writeFileSync(file, text);
    written.push(path);
  }
  return written;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  try {
    const written = await generate(ROOT);
    process.stdout.write(
      written.length > 0
        ? `Wrote, from config/product.toml:\n${written.join('\n')}\n`
        : 'Every file generated from config/product.toml is current.\n',
    );
  } catch (error) {
    process.stderr.write(`${error.message}\n`);
    process.exitCode = 1;
  }
}
