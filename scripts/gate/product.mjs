// Product identity (config/product.toml, D29): every file generated from it is current, and before a release no value is a placeholder.
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { ManifestError } from '../product/manifests.mjs';
import { CARGO_LOCK, expectedOutputs, staleLock } from '../product/outputs.mjs';
import { PLACEHOLDER_MARK, placeholders, ProductError } from '../product/read.mjs';
import { GateError, ROOT } from './process.mjs';

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

async function productCurrent() {
  const stale = await staleOutputs(ROOT);
  if (stale.length > 0) {
    throw new GateError(
      `These files are generated from config/product.toml and are out of date or missing. Run \`pnpm product:generate\` and commit the result (never edit them by hand):\n${stale.join('\n')}`,
    );
  }
}

async function noPlaceholders() {
  const { values, lines } = await remainingPlaceholders(ROOT);
  if (values.length > 0 || lines.length > 0) {
    throw new GateError(
      `A release needs the real product identity, but these values in config/product.toml are still placeholders (${PLACEHOLDER_MARK}...): ${values.join(', ')}. Fill them in and run \`pnpm product:generate\`. Generated lines that carry them:\n${lines.join('\n')}`,
    );
  }
}

export const PRODUCT_CHECKS = {
  product: productCurrent,
  'product-placeholders': noPlaceholders,
};
