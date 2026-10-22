// Product identity (config/product.toml, D29): every file generated from it is current, and before a release no value is a placeholder.
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { ManifestError } from '../product/manifests.mjs';
import { CARGO_LOCK, expectedOutputs, outputs, staleLock } from '../product/outputs.mjs';
import { PLACEHOLDER_MARK, placeholders, ProductError } from '../product/read.mjs';
import { GateError, grepRepository, ROOT } from './process.mjs';

/** The expected outputs, with the generator's own errors turned into the gate's. */
async function expected(root) {
  try {
    return await expectedOutputs(root);
  } catch (error) {
    if (error instanceof ProductError || error instanceof ManifestError) {
      throw new GateError(error.message);
    }
    throw error;
  }
}

/** The generated files under `root` that are missing or differ from what config/product.toml gives. */
export async function staleOutputs(root) {
  const { product, expected: files } = await expected(root);
  const stale = files
    .filter(({ path, text }) => {
      const file = join(root, path);
      return !existsSync(file) || readFileSync(file, 'utf8') !== text;
    })
    .map(({ path }) => path);
  // Cargo writes the lock file; it is stale when it records a workspace crate at another version than the product's.
  const lockStale = (() => {
    try {
      return staleLock(root, product).length > 0;
    } catch (error) {
      if (error instanceof ManifestError) return true;
      throw error;
    }
  })();
  return lockStale ? [...stale, CARGO_LOCK] : stale;
}

/** The values of config/product.toml, and the lines of generated files, that are still placeholders. */
export async function remainingPlaceholders(root) {
  const { product, expected: files } = await expected(root);
  const lines = files.flatMap(({ path, text }) =>
    text
      .split('\n')
      .flatMap((line, i) => (line.includes(PLACEHOLDER_MARK) ? [`${path}:${String(i + 1)}`] : [])),
  );
  return { values: placeholders(product), lines };
}

/** What every crate takes from `[workspace.package]`, which the generator writes from config/product.toml. */
const INHERITED = ['version', 'license', 'authors', 'repository', 'homepage'];

/** The crates of the workspace: the `members` of the root Cargo.toml, each a folder or a `folder/*` glob. */
function crateManifests(root) {
  const members = /^members\s*=\s*\[([^\]]*)\]/m.exec(
    readFileSync(join(root, 'Cargo.toml'), 'utf8'),
  );
  if (!members) throw new GateError('The root Cargo.toml has no `members` list.');
  return [...members[1].matchAll(/"([^"]+)"/g)].flatMap(([, member]) => {
    const folders = member.endsWith('/*')
      ? readdirSync(join(root, member.slice(0, -2)), { withFileTypes: true })
          .filter((entry) => entry.isDirectory())
          .map((entry) => `${member.slice(0, -2)}/${entry.name}`)
      : [member];
    return folders
      .map((folder) => `${folder}/Cargo.toml`)
      .filter((path) => existsSync(join(root, path)));
  });
}

/** Every crate that does not take its version, license, authors, repository or homepage from the workspace, as `path: key`. */
export function crateMetadataProblems(root) {
  return crateManifests(root).flatMap((path) => {
    const text = readFileSync(join(root, path), 'utf8');
    return INHERITED.filter(
      (key) => !new RegExp(`^${key}\\.workspace\\s*=\\s*true`, 'm').test(text),
    ).map((key) => `${path}: ${key} must be \`${key}.workspace = true\``);
  });
}

async function productCurrent() {
  const metadata = crateMetadataProblems(ROOT);
  if (metadata.length > 0) {
    throw new GateError(
      `Every crate takes this from the workspace, which config/product.toml fills in:\n${metadata.join('\n')}`,
    );
  }
  const stale = await staleOutputs(ROOT);
  if (stale.length > 0) {
    throw new GateError(
      `These files are generated from config/product.toml and are out of date or missing. Run \`pnpm product:generate\` and commit the result (never edit them by hand):\n${stale.join('\n')}`,
    );
  }
}

/** A placeholder value: the mark and the first letter of its name, so the code and docs that name the mark itself do not count. */
const PLACEHOLDER_VALUE = `${PLACEHOLDER_MARK}[A-Z]`;
/** Where a placeholder may be written by hand: the identity itself, and the tests that use one as an example. */
const PLACEHOLDER_HOMES = [':!config/product.toml', ':!scripts/**/*.test.mjs'];

/** Every line of the repository's tracked and untracked files holding a placeholder value, as `path:line:text`. */
export function placeholderHits() {
  return grepRepository(PLACEHOLDER_VALUE, ['.', ...PLACEHOLDER_HOMES]);
}

/** The `hits` outside the generated files: a placeholder copied by hand, which regenerating would not replace. */
export function handCopiedPlaceholders(hits, generated) {
  const paths = new Set(generated);
  return hits.filter((hit) => !paths.has(hit.slice(0, hit.indexOf(':'))));
}

/** What a release must still fix, one part per kind of placeholder found, or null when none is left. */
export function placeholderReport({ values, lines, copied }) {
  const parts = [];
  if (values.length > 0) {
    parts.push(
      `A release needs the real product identity, but these values in config/product.toml are still placeholders (${PLACEHOLDER_MARK}...): ${values.join(', ')}. Fill them in and run \`pnpm product:generate\`.`,
    );
  }
  if (lines.length > 0) parts.push(`Generated lines that carry them:\n${lines.join('\n')}`);
  if (copied.length > 0) {
    parts.push(
      `Placeholders written by hand elsewhere, which only config/product.toml may hold:\n${copied.join('\n')}`,
    );
  }
  return parts.length > 0 ? parts.join('\n') : null;
}

/** What a build that carries placeholders says before it goes on: what it carries, and that it is not for publishing (D49). */
export function placeholderWarning(report) {
  return `Warning: this build carries placeholder product identity, so it is for testing only; publishing it is refused (\`node scripts/gate.mjs publish\`).\n${report}`;
}

/** The report of every placeholder left in the repository, or null when none is. */
async function foundPlaceholders() {
  const { values, lines } = await remainingPlaceholders(ROOT);
  const copied = handCopiedPlaceholders(
    placeholderHits(),
    outputs(ROOT).map((output) => output.path),
  );
  return placeholderReport({ values, lines, copied });
}

async function noPlaceholders() {
  const report = await foundPlaceholders();
  if (report !== null) throw new GateError(report);
}

async function warnOfPlaceholders() {
  const report = await foundPlaceholders();
  if (report !== null) console.warn(placeholderWarning(report));
}

export const PRODUCT_CHECKS = {
  product: productCurrent,
  'product-placeholders': noPlaceholders,
  'product-placeholders-warn': warnOfPlaceholders,
};
