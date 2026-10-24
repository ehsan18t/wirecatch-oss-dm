// Every file generated from config/product.toml, and what each must contain. The generator writes these contents; the gate's `product` check compares them with the files.
import { spawnSync } from 'node:child_process';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import * as prettier from 'prettier';
import {
  editJson,
  fillTemplate,
  ManifestError,
  setPathDependencyVersions,
  setTomlKey,
  tomlString,
  withNote,
} from './manifests.mjs';
import { readProduct } from './read.mjs';
import { GENERATED_NOTE, rustSource, typescriptSource } from './sources.mjs';

export const PRODUCT_FILE = 'config/product.toml';
const TEMPLATES = 'config/templates';
/** Where the workspace's package globs are listed. */
const WORKSPACE_FILE = 'pnpm-workspace.yaml';
const TAURI_CONFIG = 'apps/desktop/src-tauri/tauri.conf.json';
/** Where Tauri finds the bridge it bundles, relative to the desktop crate; `prepare-bridge.mjs` reads the name from here. */
const SIDECAR_FOLDER = 'binaries';

function tauriConfig(json, product) {
  const main = json.app?.windows?.find((window) => window.label === 'main');
  if (!main) throw new ManifestError(`${TAURI_CONFIG} has no window labelled "main".`);
  json.productName = product.product.name;
  json.mainBinaryName = product.executables.app;
  json.identifier = product.channels.release.identifier;
  // Without a version Tauri takes the crate's, which is the workspace version written from product.toml, so the installer, the updater and the app cannot disagree.
  delete json.version;
  main.title = product.product.name;
  Object.assign(json.bundle, {
    publisher: product.publisher.legal_name,
    copyright: `${product.product.authors}. Licensed under ${product.product.license}.`,
    license: product.product.license,
    shortDescription: product.product.summary,
    longDescription: product.product.description,
  });
  json.bundle.windows.nsis.startMenuFolder = product.product.name;
}

function cargoWorkspace(text, product, path) {
  const table = '[workspace.package]';
  const fields = [
    ['version', tomlString(product.product.version)],
    ['license', tomlString(product.product.license)],
    ['authors', `[${tomlString(product.product.authors)}]`],
    ['repository', tomlString(product.links.repository)],
    ['homepage', tomlString(product.links.website)],
  ];
  const updated = fields.reduce(
    (current, [key, value]) => setTomlKey(current, path, table, [key, value]),
    text,
  );
  return setPathDependencyVersions(
    updated,
    '[workspace.dependencies]',
    product.product.version,
    path,
  );
}

/** A Cargo.toml whose one `[[bin]]` is named by `name`. */
const binaryName = (name) => (text, product, path) =>
  setTomlKey(text, path, '[[bin]]', ['name', tomlString(name(product))]);

function packageJson(json, product) {
  if ('version' in json || json.private === true) json.version = product.product.version;
  json.license = product.product.license;
}

/** The folders of the `packages` globs in pnpm-workspace.yaml (each a `folder/*`): every folder under them is a package, so one that has lost its package.json is reported missing rather than skipped. */
function packageFolders(root) {
  const manifest = current(root, WORKSPACE_FILE);
  const list = /^packages:[ \t]*\r?\n((?:(?:[ \t]*\r?\n)*[ \t]+-[ \t]+(?:\S.*)?\r?\n)+)/m.exec(
    manifest,
  );
  if (!list) throw new ManifestError(`${WORKSPACE_FILE} has no \`packages\` list.`);
  const globs = [...list[1].matchAll(/-\s+["']?([^"'\s]+)["']?/g)].map((m) => m[1]);
  return globs.flatMap((glob) => {
    if (!glob.endsWith('/*')) {
      throw new ManifestError(`The workspace glob ${glob} is not of the form folder/*.`);
    }
    const parent = glob.slice(0, -2);
    return readdirSync(join(root, parent), { withFileTypes: true })
      .filter((entry) => entry.isDirectory())
      .map((entry) => `${parent}/${entry.name}`);
  });
}

/** The workspace's package.json files: the root one, which has no version, and each package's. */
function packageManifests(root) {
  const manifests = packageFolders(root).map((folder) => `${folder}/package.json`);
  return ['package.json', ...manifests.sort()].map((path) => ({
    path,
    json:
      path === 'package.json'
        ? (json, product) => (json.license = product.product.license)
        : packageJson,
  }));
}

/** Every generated file. `source` makes a whole file; `edit` and `json` change the identity in a file that holds other things too; `template` fills a file of config/templates. */
export function outputs(root) {
  return [
    { path: 'crates/product/src/lib.rs', source: rustSource, format: 'rust' },
    { path: 'packages/product/src/index.ts', source: typescriptSource, format: 'prettier' },
    { path: 'apps/desktop/src-tauri/linux/prerm.sh', template: 'prerm.sh' },
    { path: 'apps/desktop/src-tauri/windows/hooks.nsh', template: 'hooks.nsh' },
    { path: 'SECURITY.md', template: 'SECURITY.md' },
    { path: 'Cargo.toml', edit: cargoWorkspace },
    { path: 'crates/bridge/Cargo.toml', edit: binaryName((p) => p.executables.bridge) },
    { path: 'crates/cli/Cargo.toml', edit: binaryName((p) => p.executables.cli) },
    { path: 'apps/desktop/src-tauri/Cargo.toml', edit: binaryName((p) => p.executables.app) },
    { path: TAURI_CONFIG, json: tauriConfig },
    {
      path: 'apps/desktop/src-tauri/tauri.bundle.conf.json',
      json: (json, product) =>
        (json.bundle.externalBin = [`${SIDECAR_FOLDER}/${product.executables.bridge}`]),
    },
    ...packageManifests(root),
  ];
}

/** `text` formatted by rustfmt with the workspace's settings. */
function rustfmt(text, root) {
  const result = spawnSync(
    'rustfmt',
    ['--edition', '2024', '--emit', 'stdout', '--config-path', join(root, 'rustfmt.toml')],
    { input: text, encoding: 'utf8' },
  );
  if (result.error) throw new ManifestError(`Could not start rustfmt: ${result.error.message}`);
  if (result.status !== 0)
    throw new ManifestError(`rustfmt refused the generated Rust:\n${result.stderr}`);
  return result.stdout;
}

/** `text` formatted as Prettier formats the file at `path` (ignore files aside: src-tauri is not checked, but its JSON keeps the same form). */
async function prettierFormat(text, path) {
  const options = await prettier.resolveConfig(path);
  return prettier.format(text, { ...options, filepath: path });
}

function current(root, path) {
  const file = join(root, path);
  if (!existsSync(file))
    throw new ManifestError(`${path} is missing; restore it from git, then generate again.`);
  return readFileSync(file, 'utf8');
}

/** What the file `output` describes must contain. */
async function contents(output, product, root) {
  const file = join(root, output.path);
  if (output.source) {
    const text = output.source(product);
    return output.format === 'rust' ? rustfmt(text, root) : prettierFormat(text, file);
  }
  if (output.template) {
    const name = `${TEMPLATES}/${output.template}`;
    const note = `Generated from ${name} and ${PRODUCT_FILE} by \`pnpm product:generate\`. Do not edit: change those and run it again.`;
    return withNote(fillTemplate(current(root, name), product, name), note, output.path);
  }
  if (output.json)
    return prettierFormat(
      editJson(current(root, output.path), (json) => output.json(json, product)),
      file,
    );
  return output.edit(current(root, output.path), product, output.path);
}

/** Every generated file's path and the contents it must have, from config/product.toml as it is in `root`. */
export async function expectedOutputs(root) {
  const product = readProduct(current(root, PRODUCT_FILE));
  const expected = [];
  for (const output of outputs(root)) {
    expected.push({ path: output.path, text: await contents(output, product, root) });
  }
  return { product, expected };
}

export const CARGO_LOCK = 'Cargo.lock';

/** The workspace crates (lock entries with no `source`) that Cargo.lock records at another version than `version`, as `name version`. */
export function lockMismatches(lockText, version) {
  return lockText
    .split('[[package]]')
    .slice(1)
    .filter((entry) => !/^source\s*=/m.test(entry))
    .flatMap((entry) => {
      const name = /^name\s*=\s*"([^"]+)"/m.exec(entry)?.[1];
      const recorded = /^version\s*=\s*"([^"]+)"/m.exec(entry)?.[1];
      return recorded === version ? [] : [`${name ?? '?'} ${recorded ?? '?'}`];
    });
}

/** The workspace crates Cargo.lock under `root` records at another version than the product's. */
export function staleLock(root, product) {
  return lockMismatches(current(root, CARGO_LOCK), product.product.version);
}

/** Brings Cargo.lock's workspace crates to the product version, offline and touching nothing else, so `--locked` builds keep working after a version change. */
export function refreshLock(root, product) {
  if (staleLock(root, product).length === 0) return false;
  const args = ['update', '--workspace', '--offline'];
  const result = spawnSync('cargo', args, { cwd: root, stdio: 'inherit' });
  if (result.error) throw new ManifestError(`Could not start cargo: ${result.error.message}`);
  const left = staleLock(root, product);
  if (result.status !== 0 || left.length > 0) {
    throw new ManifestError(
      `\`cargo ${args.join(' ')}\` did not bring Cargo.lock to ${product.product.version}: ${left.join(', ')}`,
    );
  }
  return true;
}

export { GENERATED_NOTE };
