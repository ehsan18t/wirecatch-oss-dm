import assert from 'node:assert/strict';
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { ROOT } from './process.mjs';
import {
  DESKTOP_CRATE,
  doctestArguments,
  TEST_FEATURES,
  testArguments,
  TOOLS,
  toolSpecs,
} from './rust.mjs';

const WORKFLOWS = join(ROOT, '.github', 'workflows');
/** The one `tool:` input a workflow may give an installer: the output of `node scripts/gate.mjs --tool-specs`. */
const TOOL_INPUT = 'tool: ${{ steps.tools.outputs.specs }}';

test('the tool specs name every tool at its version', () => {
  assert.equal(
    toolSpecs({ 'cargo-deny': '1.2.3', 'cargo-machete': '4.5.6' }),
    'cargo-deny@1.2.3,cargo-machete@4.5.6',
  );
  for (const [tool, version] of Object.entries(TOOLS)) {
    assert.ok(toolSpecs().split(',').includes(`${tool}@${version}`), tool);
  }
});

test('the workflows install the cargo tools only from the gate tool specs', () => {
  for (const file of readdirSync(WORKFLOWS)) {
    const lines = readFileSync(join(WORKFLOWS, file), 'utf8').split(/\r?\n/);
    for (const [index, line] of lines.entries()) {
      const where = `${file}:${String(index + 1)}`;
      const code = line.trim();
      if (code.startsWith('#')) continue;
      if (/^tool:/.test(code)) assert.equal(code, TOOL_INPUT, `${where} installs other tools`);
      for (const tool of Object.keys(TOOLS)) {
        assert.ok(!code.includes(tool), `${where} names ${tool}; take it from the gate tool specs`);
      }
    }
  }
});

test('the tests run in one nextest invocation that includes the desktop crate', () => {
  const args = testArguments({ desktop: true, failFast: true });
  assert.deepEqual(args, ['nextest', 'run', '--workspace', '--locked', ...TEST_FEATURES]);
});

test('the doctests run for the same crates with the same features', () => {
  for (const desktop of [true, false]) {
    const tests = testArguments({ desktop, failFast: true });
    const docs = doctestArguments({ desktop, failFast: true });
    assert.deepEqual(docs.slice(0, 2), ['test', '--doc']);
    assert.deepEqual(docs.slice(2), tests.slice(2));
  }
});

test('without the desktop crate the tests leave it out', () => {
  for (const args of [testArguments, doctestArguments]) {
    assert.deepEqual(args({ desktop: false, failFast: true }).slice(2, 5), [
      '--workspace',
      '--exclude',
      DESKTOP_CRATE,
    ]);
  }
});

test('--no-fail-fast reaches both runs', () => {
  for (const args of [testArguments, doctestArguments]) {
    assert.ok(args({ desktop: true, failFast: false }).includes('--no-fail-fast'));
  }
});
