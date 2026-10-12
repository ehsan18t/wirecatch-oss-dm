// The JavaScript checks: the generated protocol types are current, and every package is linted the one way.
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { join, relative } from 'node:path';
import { capture, GateError, pnpm, ROOT } from './process.mjs';

const GENERATED_TYPES = 'packages/protocol/src';
/** Sources that make a package subject to the shared ESLint configuration. */
const LINTED_SOURCE = /\.(ts|svelte)$/;
const ESLINT_CONFIG_PACKAGE = '@wirecatch/eslint-config';
/** A lint script that lints the whole package with no warnings allowed, then checks its formatting. */
const LINT_SCRIPT = /\beslint \. .*--max-warnings 0\b.*&& prettier --check \.$/;

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

/** The workspace package folders, from the `packages` globs in pnpm-workspace.yaml (each a `folder/*`). */
function workspacePackages() {
  const manifest = readFileSync(join(ROOT, 'pnpm-workspace.yaml'), 'utf8');
  const list = /^packages:\s*\n((?:\s+-\s+.*\n)+)/m.exec(manifest);
  if (!list) throw new GateError('pnpm-workspace.yaml has no `packages` list.');
  const globs = [...list[1].matchAll(/-\s+["']?([^"'\s]+)["']?/g)].map((m) => m[1]);
  return globs.flatMap((glob) => {
    if (!glob.endsWith('/*'))
      throw new GateError(
        `The workspace glob ${glob} is not of the form folder/*; teach workspacePackages in scripts/gate/javascript.mjs to read it.`,
      );
    const parent = glob.slice(0, -2);
    return readdirSync(join(ROOT, parent), { withFileTypes: true })
      .filter(
        (entry) =>
          entry.isDirectory() && existsSync(join(ROOT, parent, entry.name, 'package.json')),
      )
      .map((entry) => `${parent}/${entry.name}`);
  });
}

/** What is wrong with how the package in `folder` is linted, or an empty list. */
function jsPolicyProblems(folder) {
  const sources = capture('git', [
    'ls-files',
    '--cached',
    '--others',
    '--exclude-standard',
    '--',
    folder,
  ]).out.split('\n');
  if (!sources.some((file) => LINTED_SOURCE.test(file))) return [];
  const manifest = JSON.parse(readFileSync(join(ROOT, folder, 'package.json'), 'utf8'));
  const problems = [];
  const configPath = join(ROOT, folder, 'eslint.config.js');
  const config = existsSync(configPath) ? readFileSync(configPath, 'utf8') : '';
  const source = manifest.name === ESLINT_CONFIG_PACKAGE ? './index.js' : ESLINT_CONFIG_PACKAGE;
  if (!config.includes(`from '${source}'`) || !/\bwirecatch\(/.test(config)) {
    problems.push(
      `${folder}: eslint.config.js must build its configuration with \`wirecatch()\` from '${source}'.`,
    );
  }
  if (!LINT_SCRIPT.test(manifest.scripts?.lint ?? '')) {
    problems.push(
      `${folder}: package.json needs \`"lint": "eslint . --max-warnings 0 && prettier --check ."\`, so \`pnpm lint\` lints and format-checks it.`,
    );
  }
  return problems;
}

/** Every workspace package with TypeScript or Svelte sources is linted with the shared configuration, allows no warnings, and is format-checked. */
function jsPolicy() {
  const problems = workspacePackages().flatMap(jsPolicyProblems);
  if (problems.length > 0) throw new GateError(problems.join('\n'));
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
