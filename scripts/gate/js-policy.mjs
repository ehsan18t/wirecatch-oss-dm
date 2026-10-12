// `js-policy`: every workspace package, and the repository's own scripts, are linted with the shared configuration at its full strength, with no warnings allowed, and format-checked.
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { capture, GateError, ROOT } from './process.mjs';

/** Sources that make a package subject to the shared ESLint configuration. */
const LINTED_SOURCE = /\.(ts|mts|cts|svelte|js|mjs|cjs)$/;
const ESLINT_CONFIG_PACKAGE = '@wirecatch/eslint-config';
/** The one lint script: the whole package, no warnings allowed, then its formatting. Anything added to it (a `--rule`, an `|| true`) could weaken it, so it is compared exactly. */
const LINT_SCRIPT = 'eslint . --max-warnings 0 && prettier --check .';
/** The root package's lint script: every package's, then the repository's own scripts, which the root eslint.config.js covers. */
const ROOT_LINT_SCRIPT =
  'pnpm -r --if-present lint && eslint scripts --max-warnings 0 && prettier --check scripts';
/** The root package's sources: what its lint script lints, and its configuration. */
const ROOT_SOURCES = /^(scripts\/|eslint\.config\.js$)/;
/**
 * The only tracked sources a package configuration may ignore, each for a reason: the types ts-rs generates from crates/protocol (their source is linted as Rust, and tsc checks them), and the file the rule tests lint with code of their own.
 */
const MAY_BE_IGNORED = [
  'packages/protocol/src/generated/',
  'packages/eslint-config/rules/fixtures/',
];
/** JavaScript every package may lint without type information (`untyped` in wirecatch()), so the baseline for a JavaScript file is the untyped one. */
const UNTYPED_JAVASCRIPT = ['**/*.js', '**/*.cjs'];

/** The workspace package folders, from the `packages` globs in pnpm-workspace.yaml (each a `folder/*`). */
function workspacePackages() {
  const manifest = readFileSync(join(ROOT, 'pnpm-workspace.yaml'), 'utf8');
  const list = /^packages:\s*\n((?:\s+-\s+.*\n)+)/m.exec(manifest);
  if (!list) throw new GateError('pnpm-workspace.yaml has no `packages` list.');
  const globs = [...list[1].matchAll(/-\s+["']?([^"'\s]+)["']?/g)].map((m) => m[1]);
  return globs.flatMap((glob) => {
    if (!glob.endsWith('/*')) {
      throw new GateError(
        `The workspace glob ${glob} is not of the form folder/*; teach workspacePackages in scripts/gate/js-policy.mjs to read it.`,
      );
    }
    const parent = glob.slice(0, -2);
    return readdirSync(join(ROOT, parent), { withFileTypes: true })
      .filter(
        (entry) =>
          entry.isDirectory() && existsSync(join(ROOT, parent, entry.name, 'package.json')),
      )
      .map((entry) => `${parent}/${entry.name}`);
  });
}

/** The package's files, relative to its folder, tracked and untracked (not ignored). */
function packageFiles(folder) {
  const prefix = folder === '.' ? '' : `${folder}/`;
  const files = capture('git', [
    'ls-files',
    '--cached',
    '--others',
    '--exclude-standard',
    '--',
    folder,
  ])
    .out.split('\n')
    .filter((file) => file !== '' && file.startsWith(prefix))
    .map((file) => file.slice(prefix.length));
  return folder === '.' ? files.filter((file) => ROOT_SOURCES.test(file)) : files;
}

/** What is wrong with the package's configuration file and lint script, or an empty list. */
function setupProblems(folder, manifest) {
  const script = folder === '.' ? ROOT_LINT_SCRIPT : LINT_SCRIPT;
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
  if (manifest.scripts?.lint !== script) {
    problems.push(
      `${where}: package.json needs exactly \`"lint": "${script}"\`, so \`pnpm lint\` lints and format-checks it with nothing switched off.`,
    );
  }
  return problems;
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
      if (!MAY_BE_IGNORED.some((allowed) => path.startsWith(allowed))) {
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

/** Every workspace package with sources ESLint reads is linted with the shared configuration at full strength, allows no warnings, and is format-checked. Package layers may add rules; they may not weaken one. */
export async function jsPolicy() {
  const problems = [];
  // The root package lints the repository's own scripts.
  for (const folder of [...workspacePackages(), '.']) {
    const files = packageFiles(folder);
    if (!files.some((file) => LINTED_SOURCE.test(file))) continue;
    const manifest = JSON.parse(readFileSync(join(ROOT, folder, 'package.json'), 'utf8'));
    const setup = setupProblems(folder, manifest);
    problems.push(...setup, ...(setup.length === 0 ? await configProblems(folder, files) : []));
  }
  if (problems.length > 0) throw new GateError(problems.join('\n'));
}
