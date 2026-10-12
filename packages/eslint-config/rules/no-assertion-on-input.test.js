import { after, describe, it } from 'node:test';
import { fileURLToPath } from 'node:url';
import { RuleTester } from 'eslint';
import ts from 'typescript-eslint';
import { noAssertionOnInput } from './no-assertion-on-input.js';

RuleTester.describe = describe;
RuleTester.it = it;
RuleTester.afterAll = after;

// Typed rules need a program: the tests lint fixtures/file.ts with the code each one gives.
const fixtures = fileURLToPath(new URL('fixtures/', import.meta.url));
const tester = new RuleTester({
  languageOptions: {
    parser: ts.parser,
    parserOptions: { project: './tsconfig.json', tsconfigRootDir: fixtures },
  },
});
const filename = fileURLToPath(new URL('fixtures/file.ts', import.meta.url));
const DECLARATIONS = `
interface Message { type: string }
declare function isMessage(raw: unknown): raw is Message;
declare const browserEvent: Event;
declare const storage: { get(keys: string[]): Promise<Record<string, any>> };
`;
const valid = (code) => ({ code: DECLARATIONS + code, filename });
const invalid = (code) => ({
  code: DECLARATIONS + code,
  filename,
  errors: [{ messageId: 'assertion' }],
});

tester.run('no-assertion-on-input', noAssertionOnInput, {
  valid: [
    // Checked with a guard, not asserted.
    valid(
      'export function asMessage(raw: unknown): Message | null { return isMessage(raw) ? raw : null; }',
    ),
    // `as const` says nothing about the value.
    valid(
      'export function kinds(raw: unknown) { return raw === null ? (["a", "b"] as const) : <const>["c"]; }',
    ),
    // A value whose type is known may be narrowed; that is a different decision.
    valid('export const pointer = browserEvent as PointerEvent;'),
    valid(
      'export function f(raw: unknown) { const inner = (e: Event) => e as KeyboardEvent; return [raw, inner]; }',
    ),
    valid('export const first = (list: string[]) => list[0]!;'),
  ],
  invalid: [
    invalid('export function asMessage(raw: unknown) { return raw as Message; }'),
    invalid('export function asMessage(raw: unknown) { return <Message>raw; }'),
    invalid('export function asMessage(raw: unknown) { return raw!; }'),
    // `any`, the type the browser's listener arguments and JSON.parse leave.
    invalid('export function onMessage(raw: any) { return raw as Message; }'),
    invalid('export const parse = (text: string) => JSON.parse(text) as Message;'),
    invalid(
      'export async function on() { const items = await storage.get(["on"]); return items.on as boolean; }',
    ),
    // Parameters that are not plain names.
    invalid('export function first({ raw }: { raw: unknown }) { return raw as string; }'),
    invalid('export function all(...raw: unknown[]) { return raw[0] as string; }'),
    // Anywhere, not only in a function that reads input.
    invalid('declare const stored: unknown; export const settings = stored as Message;'),
  ],
});
