import assert from 'node:assert/strict';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, beforeEach, test } from 'node:test';
import ts from 'typescript';
import { validatorsSource } from './validators.mjs';

let folder;

beforeEach(() => {
  folder = mkdtempSync(join(tmpdir(), 'validators-'));
});

afterEach(() => {
  rmSync(folder, { recursive: true, force: true });
});

/** Writes generated type files, as ts-rs writes them, and loads the guards made from them. */
async function guardsFor(types, options) {
  for (const [name, body] of Object.entries(types)) {
    writeFileSync(join(folder, `${name}.ts`), `export type ${name} = ${body};\n`);
  }
  const source = validatorsSource(folder, options);
  const { outputText } = ts.transpileModule(source, {
    compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 },
  });
  const guards = await import(`data:text/javascript,${encodeURIComponent(outputText)}`);
  return { source, guards };
}

test('an object needs every field it names, of the right type, and may carry others', async () => {
  const { guards } = await guardsFor({
    Settings: '{ on: boolean, name: string | null, size?: number, sites: Array<string> }',
  });
  const { isSettings } = guards;
  assert.equal(isSettings({ on: true, name: null, sites: ['a'] }), true);
  assert.equal(isSettings({ on: true, name: 'x', size: 3, sites: [], later: 1 }), true);
  assert.equal(isSettings({ on: true, sites: [] }), false, 'a missing field');
  assert.equal(isSettings({ on: 'yes', name: null, sites: [] }), false, 'a wrong type');
  assert.equal(isSettings({ on: true, name: null, size: 'big', sites: [] }), false);
  assert.equal(isSettings({ on: true, name: null, sites: [1] }), false, 'a wrong list item');
  assert.equal(isSettings([true]), false);
  assert.equal(isSettings(null), false);
});

test('a union is one of its members, told apart by the tag', async () => {
  const { guards } = await guardsFor({
    Kind: '"a" | "b"',
    Event: '{ "type": "plain" } | { "type": "with", "data": { kind: Kind, items: Array<Kind> } }',
  });
  const { isEvent, isKind } = guards;
  assert.equal(isKind('a'), true);
  assert.equal(isKind('c'), false);
  assert.equal(isEvent({ type: 'plain' }), true);
  assert.equal(isEvent({ type: 'with', data: { kind: 'b', items: ['a'] } }), true);
  assert.equal(isEvent({ type: 'with', data: { kind: 'c', items: [] } }), false);
  assert.equal(isEvent({ type: 'with', data: { kind: 'a', items: ['x'] } }), false);
  assert.equal(isEvent({ type: 'other' }), false);
});

test('a type with type parameters gets no guard, nor does a type that uses one', async () => {
  writeFileSync(join(folder, 'Pair.ts'), 'export type Pair<A, B> = { first: A, second: B };\n');
  const { source, guards } = await guardsFor(
    { Uses: '{ pair: Pair<string, number> }', Plain: '{ n: number }' },
    { withoutGuard: ['Pair', 'Uses'] },
  );
  assert.doesNotMatch(source, /isPair|isUses/);
  assert.equal(guards.isPlain({ n: 1 }), true);
});

test('a type left without a guard that is not listed as one fails the generation', async () => {
  writeFileSync(join(folder, 'Pair.ts'), 'export type Pair<A, B> = { first: A, second: B };\n');
  await assert.rejects(
    guardsFor({ Message: '{ pair: Pair<string, number> }' }, { withoutGuard: ['Pair'] }),
    /Message/,
  );
});

test('every command in the command map gets a guard for its answer', async () => {
  writeFileSync(
    join(folder, 'Command.ts'),
    'export type Command<A, O> = { args: A, output: O };\n',
  );
  const { guards } = await guardsFor({
    Row: '{ id: number }',
    CommandMap: '{ list: Command<null, Array<Row>>, find: Command<Row, Row | null> }',
  });
  const outputs = guards.COMMAND_OUTPUTS;
  assert.equal(outputs.list([{ id: 1 }]), true);
  assert.equal(outputs.list([{ id: 'x' }]), false);
  assert.equal(outputs.find(null), true);
  assert.equal(outputs.find({}), false);
});

test('a branded id is checked as the value it brands, and a type naming it uses its guard', async () => {
  const { source, guards } = await guardsFor({
    DownloadId: 'number & { readonly __brand: "DownloadId" }',
    Row: '{ id: DownloadId, ids: Array<DownloadId> }',
  });
  assert.match(source, /isDownloadId\(value1\["id"\]\)/);
  assert.equal(guards.isDownloadId(3), true);
  assert.equal(guards.isDownloadId(0), true);
  assert.equal(guards.isDownloadId('3'), false);
  for (const notAnId of [-1, 1.5, Number.NaN, Infinity, 2 ** 53]) {
    assert.equal(guards.isDownloadId(notAnId), false, String(notAnId));
  }
  assert.equal(guards.isRow({ id: 3, ids: [4] }), true);
  assert.equal(guards.isRow({ id: 3, ids: ['4'] }), false);
});
