#!/usr/bin/env node
// Reports outdated dependencies as Markdown: the direct dependencies of every Cargo workspace crate, the pnpm workspace's packages, and the actions the workflows pin. Nothing is changed and nothing opens a pull request or an issue (D28); the maintainer reads the report and updates on purpose.
//
// The report goes to $GITHUB_STEP_SUMMARY when it is set (the scheduled workflow, .github/workflows/outdated.yml), and to standard output always. An outdated dependency is a finding, not a failure: the script exits non-zero only when a section could not be produced, and still reports the others.
//
// GITHUB_TOKEN, when set, is sent with the GitHub API requests that look up the actions' latest releases, for the higher rate limit.

import { appendFileSync, readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { capture, cargoMetadata, ROOT } from './gate/process.mjs';

const WORKFLOWS = join(ROOT, '.github', 'workflows');

/** One `cargo update --dry-run --verbose` line: a package that would move, or one that stays although a newer version exists. */
const CARGO_LINE =
  /^\s*(?:Updating|Unchanged)\s+(\S+) v(\d\S*)(?: -> v(\d\S*))?(?: \(available: v(\d\S*?)\))?\s*$/;
/** A step's `uses: owner/repo[/path]@<sha> # <version>`. */
const PINNED_ACTION = /uses:\s*([\w.-]+\/[\w.-]+)(?:\/\S*)?@[0-9a-f]{40}\s*#\s*(v?\d\S*)/;

function parseVersion(text) {
  const [major = 0, minor = 0, patch = 0] = text
    .replace(/^v/, '')
    .split(/[-+]/)[0]
    .split('.')
    .map(Number);
  return { major, minor, patch };
}

function compareVersions(a, b) {
  const x = parseVersion(a);
  const y = parseVersion(b);
  return x.major - y.major || x.minor - y.minor || x.patch - y.patch;
}

/** Whether Cargo's default `^` requirement on `locked` accepts `candidate`: same major, or same minor below 1.0, or same patch below 0.1. */
function semverCompatible(locked, candidate) {
  const a = parseVersion(locked);
  const b = parseVersion(candidate);
  if (a.major !== b.major) return false;
  if (a.major > 0) return true;
  if (a.minor !== b.minor) return false;
  return a.minor > 0 || a.patch === b.patch;
}

function table(header, rows) {
  const line = (cells) => `| ${cells.join(' | ')} |`;
  return [line(header), line(header.map(() => '---')), ...rows.map(line)].join('\n');
}

/** Each direct dependency of a workspace crate, as `name@version`, with the crates that use it. */
function directCargoDependencies() {
  const metadata = cargoMetadata({ deps: true });
  const packages = new Map(metadata.packages.map((pkg) => [pkg.id, pkg]));
  const members = new Set(metadata.workspace_members);
  const direct = new Map();
  for (const node of metadata.resolve.nodes) {
    if (!members.has(node.id)) continue;
    const member = packages.get(node.id).name;
    for (const dep of node.deps) {
      if (members.has(dep.pkg)) continue;
      const pkg = packages.get(dep.pkg);
      const key = `${pkg.name}@${pkg.version}`;
      const users = direct.get(key) ?? new Set();
      users.add(member);
      direct.set(key, users);
    }
  }
  return direct;
}

function cargoSection() {
  const direct = directCargoDependencies();
  // cargo writes its status lines, the ones read here, to standard error.
  const { err } = capture('cargo', ['update', '--dry-run', '--verbose']);
  const rows = [];
  for (const line of err.split(/\r?\n/)) {
    const match = CARGO_LINE.exec(line);
    if (!match) continue;
    const [, name, locked, updatesTo, available] = match;
    const users = direct.get(`${name}@${locked}`);
    if (!users) continue;
    const compatible = [];
    if (updatesTo) compatible.push(updatesTo);
    let incompatible = '';
    if (available && semverCompatible(locked, available)) {
      compatible.push(`${available} (held back by the Rust version or a pinned requirement)`);
    } else if (available) {
      incompatible = available;
    }
    rows.push([name, locked, compatible.join(', '), incompatible, [...users].sort().join(', ')]);
  }
  rows.sort((a, b) => a[0].localeCompare(b[0]));
  const intro = `${String(rows.length)} of ${String(direct.size)} direct dependencies have a newer version. A compatible one is a \`cargo update\` away; an incompatible one needs its requirement raised in Cargo.toml.`;
  if (rows.length === 0) return `${intro}\n`;
  return `${intro}\n\n${table(['Crate', 'Locked', 'Newer compatible', 'Newer incompatible', 'Used by'], rows)}\n`;
}

function npmSection() {
  // pnpm exits 1 when something is outdated; that is the answer, not a failure.
  const { out } = capture('pnpm', ['outdated', '--recursive', '--format', 'json'], {
    allowed: [0, 1],
  });
  const outdated = out.trim() ? JSON.parse(out) : {};
  const rows = Object.entries(outdated)
    .map(([name, entry]) => [
      entry.isDeprecated ? `${name} (deprecated)` : name,
      entry.current,
      entry.wanted,
      entry.latest,
      entry.dependencyType,
      entry.dependentPackages.map((pkg) => pkg.name).join(', '),
    ])
    .sort((a, b) => a[0].localeCompare(b[0]));
  if (rows.length === 0) return 'Every package is on its latest version.\n';
  return `${String(rows.length)} packages have a newer version. *Wanted* is the newest the declared range accepts.\n\n${table(['Package', 'Current', 'Wanted', 'Latest', 'Type', 'Used by'], rows)}\n`;
}

function pinnedActions() {
  const pinned = new Map();
  for (const file of readdirSync(WORKFLOWS).filter((name) => /\.ya?ml$/.test(name))) {
    for (const line of readFileSync(join(WORKFLOWS, file), 'utf8').split(/\r?\n/)) {
      const match = PINNED_ACTION.exec(line);
      if (match) pinned.set(`${match[1]}@${match[2]}`, { repo: match[1], version: match[2] });
    }
  }
  return [...pinned.values()];
}

async function latestRelease(repo) {
  const headers = { Accept: 'application/vnd.github+json', 'User-Agent': 'wirecatch-outdated' };
  if (process.env.GITHUB_TOKEN) headers.Authorization = `Bearer ${process.env.GITHUB_TOKEN}`;
  const response = await fetch(`https://api.github.com/repos/${repo}/releases/latest`, { headers });
  if (!response.ok) {
    throw new Error(
      `the latest release of ${repo} could not be read (HTTP ${String(response.status)})`,
    );
  }
  return (await response.json()).tag_name;
}

async function actionsSection() {
  const actions = pinnedActions();
  const latest = await Promise.all(actions.map(({ repo }) => latestRelease(repo)));
  const rows = actions
    .map((action, index) => [action.repo, action.version, latest[index]])
    .filter(([, version, release]) => compareVersions(release, version) > 0)
    .sort((a, b) => a[0].localeCompare(b[0]));
  const intro = `${String(rows.length)} of ${String(actions.length)} pinned actions have a newer release. Updating one means pinning the new release's commit SHA and changing the version comment beside it.`;
  if (rows.length === 0) return `${intro}\n`;
  return `${intro}\n\n${table(['Action', 'Pinned', 'Latest release'], rows)}\n`;
}

/** Runs one section; a section that fails says why in the report instead of hiding the others. */
async function section(title, produce) {
  try {
    return { text: `## ${title}\n\n${await produce()}`, failed: false };
  } catch (error) {
    return { text: `## ${title}\n\nNot reported: ${error.message}\n`, failed: true };
  }
}

async function main() {
  const sections = [
    await section('Cargo', cargoSection),
    await section('npm', npmSection),
    await section('GitHub Actions', actionsSection),
  ];
  const report = `# Outdated dependencies\n\n${sections.map((part) => part.text).join('\n')}`;
  console.log(report);
  if (process.env.GITHUB_STEP_SUMMARY) appendFileSync(process.env.GITHUB_STEP_SUMMARY, report);
  if (sections.some((part) => part.failed)) process.exitCode = 1;
}

await main();
