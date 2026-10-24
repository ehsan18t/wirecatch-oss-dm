// `js-policy`: every workspace package, and the repository's own scripts, are linted with the shared configuration at its full strength, with no warnings allowed, and format-checked, all by the one root `lint` script.
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { capture, GateError, ROOT } from './process.mjs';

/** Sources that make a package subject to the shared ESLint configuration. */
const LINTED_SOURCE = /\.(?:ts|mts|cts|svelte|js|mjs|cjs)$/;
const ESLINT_CONFIG_PACKAGE = '@wirecatch/eslint-config';
/** The folder of the repository's own scripts, which the root lints and formats besides the workspace packages. */
const ROOT_FOLDERS = ['scripts'];
/** What the root lints: its scripts and the eslint.config.js that covers them. */
const ROOT_LINTED = [...ROOT_FOLDERS, 'eslint.config.js'];
/** The root package's sources: what its lint script lints, and its configuration. */
const ROOT_SOURCES = /^(?:scripts\/|eslint\.config\.js$)/;
/** Scripts only the root has: lint and format run once over the whole repository, so a package has no way of its own to run them. */
const ROOT_ONLY_SCRIPTS = ['lint', 'format'];
/** Prettier, run from the root, reads only the root's ignore files, so one anywhere else would be silently unused. */
const IGNORE_FILE = /(?:^|\/)\.(?:gitignore|prettierignore)$/;
/**
 * The only tracked sources a package configuration may ignore, each for a reason: the types ts-rs generates from crates/protocol (their source is linted as Rust, and tsc checks them), and the file the rule tests lint with code of their own.
 */
const MAY_BE_IGNORED = [
  'packages/protocol/src/generated/',
  'packages/eslint-config/rules/fixtures/',
];
/** JavaScript every package may lint without type information (`untyped` in wirecatch()), so the baseline for a JavaScript file is the untyped one. */
const UNTYPED_JAVASCRIPT = ['**/*.js', '**/*.cjs'];

/** The folders that hold the workspace packages, from the `packages` globs in pnpm-workspace.yaml (each a `folder/*`). */
function workspaceParents() {
  const manifest = readFileSync(join(ROOT, 'pnpm-workspace.yaml'), 'utf8');
  const list = /^packages:[ \t]*\r?\n((?:(?:[ \t]*\r?\n)*[ \t]+-[ \t]+(?:\S.*)?\r?\n)+)/m.exec(
    manifest,
  );
  if (!list) throw new GateError('pnpm-workspace.yaml has no `packages` list.');
  const globs = [...list[1].matchAll(/-\s+["']?([^"'\s]+)["']?/g)].map((m) => m[1]);
  return globs.map((glob) => {
    if (!glob.endsWith('/*')) {
      throw new GateError(
        `The workspace glob ${glob} is not of the form folder/*; teach workspaceParents in scripts/gate/js-policy.mjs to read it.`,
      );
    }
    return glob.slice(0, -2);
  });
}

/** The workspace package folders. */
function workspacePackages() {
  return workspaceParents().flatMap((parent) =>
    readdirSync(join(ROOT, parent), { withFileTypes: true })
      .filter(
        (entry) =>
          entry.isDirectory() && existsSync(join(ROOT, parent, entry.name, 'package.json')),
      )
      .map((entry) => `${parent}/${entry.name}`),
  );
}

/**
 * The root `lint` and `format` scripts. `lint` runs ESLint in every workspace package, then on the root's own sources, no warnings allowed, then Prettier once over all of them; `format` writes what that Prettier run checks. Anything added to `lint` (a `--rule`, an `|| true`) could weaken it, and a folder left out of either would go unchecked or unformatted, so both are compared exactly.
 *
 * ESLint runs once per package, in that package's folder, never once over the whole repository: in one process, linting a file of one package first takes the type information of `.svelte` files in another (D42).
 */
function rootScripts() {
  const formatted = [...workspaceParents(), ...ROOT_LINTED].join(' ');
  const root = ROOT_LINTED.join(' ');
  return {
    lint: `pnpm -r exec eslint . --max-warnings 0 && eslint ${root} --max-warnings 0 && prettier --check ${formatted}`,
    format: `prettier --write ${formatted}`,
  };
}

/** Files git tracks or would track (untracked and not ignored) under the given paths, read NUL-separated so a name git would quote comes through as it is. */
function gitFiles(...paths) {
  return capture('git', [
    'ls-files',
    '-z',
    '--cached',
    '--others',
    '--exclude-standard',
    '--',
    ...paths,
  ])
    .out.split('\0')
    .filter((file) => file !== '');
}

/** The package's files, relative to its folder, tracked and untracked (not ignored). */
function packageFiles(folder) {
  const prefix = folder === '.' ? '' : `${folder}/`;
  const files = gitFiles(folder)
    .filter((file) => file.startsWith(prefix))
    .map((file) => file.slice(prefix.length));
  return folder === '.' ? files.filter((file) => ROOT_SOURCES.test(file)) : files;
}

/** What is wrong with the package's configuration file and scripts, or an empty list. */
function setupProblems(folder, manifest) {
  const where = folder === '.' ? 'the root package' : folder;
  const problems = [];
  const configPath = join(ROOT, folder, 'eslint.config.js');
  const config = existsSync(configPath) ? readFileSync(configPath, 'utf8') : '';
  const source = manifest.name === ESLINT_CONFIG_PACKAGE ? './index.js' : ESLINT_CONFIG_PACKAGE;
  if (!config.includes(`from '${source}'`) || !/\bwirecatch\(/.test(config)) {
    problems.push(
      `${where}: eslint.config.js must build its configuration with \`wirecatch()\` from '${source}'.`,
    );
  }
  if (folder === '.') {
    for (const [name, script] of Object.entries(rootScripts())) {
      if (manifest.scripts?.[name] !== script) {
        problems.push(
          `the root package: package.json needs exactly \`"${name}": "${script}"\`, so \`pnpm ${name}\` covers every package and the root's scripts with nothing switched off.`,
        );
      }
    }
  } else {
    const own = ROOT_ONLY_SCRIPTS.filter((name) => manifest.scripts?.[name] !== undefined);
    for (const name of own) {
      problems.push(
        `${where}: package.json has a \`${name}\` script, but ${name} runs once from the root (\`pnpm ${name}\`); remove it.`,
      );
    }
  }
  return problems;
}

/** Ignore files inside the folders the root `lint` script format-checks, which Prettier run from the root never reads. */
function strayIgnoreFiles() {
  return gitFiles(...workspaceParents(), ...ROOT_FOLDERS)
    .filter((file) => IGNORE_FILE.test(file))
    .map(
      (file) =>
        `${file}: Prettier runs from the root and reads only the root .gitignore and .prettierignore; move its rules there, with the folder in front.`,
    );
}

/** How a baseline rule setting differs in `effective`, or null when it does not: a lower severity or other options. A higher severity is no difference. */
function weakening(name, baseline, effective) {
  const [level, ...settings] = baseline;
  const [actual = 0, ...actualSettings] = effective ?? [];
  if (actual < level) return `${name} is ${['off', 'a warning', 'an error'][actual]}`;
  if (JSON.stringify(settings) !== JSON.stringify(actualSettings))
    return `${name} has other options`;
  return null;
}

/** The ESLint API, loaded only when this check runs (it needs `pnpm install`), with the shared configuration. */
async function eslintApi(folder) {
  const { ESLint } = await import('eslint');
  const { wirecatch } = await import(ESLINT_CONFIG_PACKAGE);
  const cwd = join(ROOT, folder);
  const effective = new ESLint({ cwd });
  const baseline = new ESLint({
    cwd,
    overrideConfigFile: true,
    overrideConfig: wirecatch({ tsconfigRootDir: cwd, globals: {}, untyped: UNTYPED_JAVASCRIPT }),
  });
  return { effective, baseline, cwd };
}

/** Where a file is, from the repository root. */
const fromRoot = (folder, file) => (folder === '.' ? file : `${folder}/${file}`);

/** How the configuration ESLint computes for one file differs from the baseline's: its rules and its linter options. */
function differences(expected, actual) {
  const rules = Object.entries(expected.rules)
    .filter(([, setting]) => setting[0] > 0)
    .map(([name, setting]) => weakening(name, setting, actual.rules[name]));
  const options =
    JSON.stringify(actual.linterOptions) === JSON.stringify(expected.linterOptions)
      ? null
      : `the linter options are ${JSON.stringify(actual.linterOptions)}`;
  return [...rules, options].filter((difference) => difference !== null);
}

/** For every source file of the package, how its own configuration differs from the shared baseline there, and every source it ignores that it may not. Tests and tooling are compared too, since the baseline loosens them in the same way. */
async function weakenedRules(folder, files) {
  const { effective, baseline, cwd } = await eslintApi(folder);
  const problems = [];
  for (const file of files.filter((f) => LINTED_SOURCE.test(f))) {
    const path = fromRoot(folder, file);
    const actual = await effective.calculateConfigForFile(join(cwd, file));
    if (actual === undefined) {
      if (MAY_BE_IGNORED.every((allowed) => !path.startsWith(allowed))) {
        problems.push(
          `${path}: ignored by its package's ESLint configuration; only the folders in MAY_BE_IGNORED (scripts/gate/js-policy.mjs) may be.`,
        );
      }
      continue;
    }
    const different = differences(await baseline.calculateConfigForFile(join(cwd, file)), actual);
    if (different.length > 0)
      problems.push(`${path}: differs from the shared baseline: ${different.join('; ')}.`);
  }
  return problems;
}

/** The weakened rules of the package, or why its configuration does not load (wirecatch() refuses options that would weaken it). */
async function configProblems(folder, files) {
  try {
    return await weakenedRules(folder, files);
  } catch (error) {
    if (!(error instanceof Error)) throw error;
    return [`${folder}: its ESLint configuration does not load: ${error.message}`];
  }
}

/** Every workspace package with sources ESLint reads is linted with the shared configuration at full strength, allows no warnings, and is format-checked, by the root `lint` script. Package layers may add rules; they may not weaken one. */
export async function jsPolicy() {
  const problems = strayIgnoreFiles();
  // The root package lints the repository's own scripts.
  for (const folder of [...workspacePackages(), '.']) {
    const files = packageFiles(folder);
    if (files.every((file) => !LINTED_SOURCE.test(file))) continue;
    const manifest = JSON.parse(readFileSync(join(ROOT, folder, 'package.json'), 'utf8'));
    const setup = setupProblems(folder, manifest);
    problems.push(...setup, ...(setup.length === 0 ? await configProblems(folder, files) : []));
  }
  if (problems.length > 0) throw new GateError(problems.join('\n'));
}
