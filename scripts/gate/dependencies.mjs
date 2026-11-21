// Require one version requirement for each crates.io dependency shared by workspace members.
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { parse } from 'smol-toml';
import { GateError, ROOT } from './process.mjs';
import { crateManifests } from './product.mjs';

const SECTIONS = ['dependencies', 'dev-dependencies', 'build-dependencies'];

function nativeDependency(declaration) {
  return typeof declaration === 'string' ? { version: declaration } : declaration;
}

function registry(declaration) {
  return Object.keys(declaration).every((key) => !['path', 'git', 'registry'].includes(key));
}

function declarationLine(text, key, section, target) {
  const lines = text.split('\n');
  const keyPattern = new RegExp(String.raw`^\s*["']?${key}["']?\s*(?:=|\.)`);
  const prefix = target
    ? `target.${target.replaceAll('"', '').replaceAll("'", '')}.${section}`
    : section;
  let sectionLine = -1;
  for (const [index, line] of lines.entries()) {
    const header = /^\s*\[([^\]]+)\]/u.exec(line);
    if (header) {
      const table = header[1];
      const normalized = table.replaceAll('"', '').replaceAll("'", '');
      sectionLine = normalized === prefix ? index : -1;
      if (normalized === `${prefix}.${key}`) return index + 1;
    }
    if (sectionLine >= 0 && keyPattern.test(line)) return index + 1;
  }
  return null;
}

function declarations(manifest) {
  const scopes = [[null, manifest], ...Object.entries(manifest.target ?? {})];
  return scopes.flatMap(([target, scope]) =>
    SECTIONS.flatMap((section) =>
      Object.entries(scope[section] ?? {}).map(([key, raw]) => ({
        target,
        section,
        key,
        member: nativeDependency(raw),
      })),
    ),
  );
}

function registryNames(dependencies) {
  return new Set(
    Object.entries(dependencies).flatMap(([key, value]) => {
      const declaration = nativeDependency(value);
      return registry(declaration) ? [declaration.package ?? key] : [];
    }),
  );
}

/** The direct declarations that should inherit their version, as manifest locations. */
export function dependencyProblems(workspace, manifests) {
  const inherited = parse(workspace).workspace?.dependencies ?? {};
  const shared = registryNames(inherited);
  const membersByPackage = new Map();
  const direct = [];
  for (const [path, text] of manifests) {
    for (const { target, section, key, member } of declarations(parse(text))) {
      const value = member.workspace && Object.hasOwn(inherited, key) ? inherited[key] : member;
      const dependency = nativeDependency(value);
      if (!registry(dependency)) continue;
      const name = dependency.package ?? key;
      const members = membersByPackage.get(name) ?? new Set();
      members.add(path);
      membersByPackage.set(name, members);
      if (!member.workspace && 'version' in member) {
        direct.push({ name, path, text, key, section, target });
      }
    }
  }
  return direct
    .filter(({ name }) => shared.has(name) || membersByPackage.get(name).size > 1)
    .map(({ path, text, key, section, target }) => {
      const line = declarationLine(text, key, section, target);
      return line
        ? `${path}:${line} (${key})`
        : `${path} (${[target, section, key].filter(Boolean).join('.')})`;
    });
}

export function checkDependencies() {
  const read = (path) => readFileSync(join(ROOT, path), 'utf8');
  const manifests = new Map(crateManifests(ROOT).map((path) => [path, read(path)]));
  const problems = dependencyProblems(read('Cargo.toml'), manifests);
  if (problems.length) {
    throw new GateError(
      `Shared crates.io dependencies must inherit their versions from [workspace.dependencies]:\n${problems.join('\n')}`,
    );
  }
}
