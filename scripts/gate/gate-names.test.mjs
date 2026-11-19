// Every check or group a workflow or a package script asks the gate for exists: a name left behind when a group is removed fails here, not in the next release run.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { ROOT } from './process.mjs';

/** The checks and groups `--list` names. */
function known() {
  const listed = execFileSync(process.execPath, [join(ROOT, 'scripts/gate.mjs'), '--list'], {
    encoding: 'utf8',
  });
  const names = new Set();
  for (const line of listed.split('\n')) {
    const [head, rest] = line.split(/:\s*/, 2);
    if (rest === undefined) continue;
    if (head === 'Checks') rest.split(/,\s*/).forEach((name) => names.add(name));
    else names.add(head.trim());
  }
  return names;
}

/** The names each `gate.mjs NAME...` in `text` asks for; a name built from a workflow's `${{ }}` is left out. */
export function askedNames(text) {
  return [...text.matchAll(/gate\.mjs((?:[ \t]+[\w-]+(?![\w$-]))+)/g)].flatMap((match) =>
    match[1]
      .trim()
      .split(/[ \t]+/)
      .filter((name) => !name.startsWith('-')),
  );
}

test('the names in a gate call are read', () => {
  assert.deepEqual(askedNames('run: node scripts/gate.mjs release-build --no-fail-fast'), [
    'release-build',
  ]);
  assert.deepEqual(askedNames('"x": "node ../../scripts/gate.mjs publish && wxt"'), ['publish']);
  assert.deepEqual(askedNames('run: node scripts/gate.mjs rust-${{ matrix.part }}'), []);
});

test('every check or group a workflow or package script asks for exists', () => {
  const names = known();
  const files = [
    ...readdirSync(join(ROOT, '.github/workflows')).map((file) => `.github/workflows/${file}`),
    'package.json',
    ...['apps', 'packages'].flatMap((folder) =>
      readdirSync(join(ROOT, folder)).map((name) => `${folder}/${name}/package.json`),
    ),
  ];
  const unknown = files.flatMap((file) => {
    let text;
    try {
      text = readFileSync(join(ROOT, file), 'utf8');
    } catch {
      return [];
    }
    return askedNames(text)
      .filter((name) => !names.has(name))
      .map((name) => `${file}: ${name}`);
  });
  assert.deepEqual(unknown, []);
});
