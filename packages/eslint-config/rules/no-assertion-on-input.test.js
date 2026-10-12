import { after, describe, it } from 'node:test';
import { RuleTester } from 'eslint';
import { noAssertionOnInput } from './no-assertion-on-input.js';
import { filename, typedRuleTester } from './typed-rule-tester.js';

RuleTester.describe = describe;
RuleTester.it = it;
RuleTester.afterAll = after;

const tester = typedRuleTester();
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
    valid('export function f(raw: { type: string }) { return raw as Message; }'),
    valid('export function f<T extends Event>(raw: T) { return raw as KeyboardEvent; }'),
    // An assertion to a type that is itself unchecked claims nothing: the fields are read and checked one by one.
    valid(
      'export function f(raw: unknown) { if (typeof raw !== "object" || raw === null) return null; const fields = raw as Record<string, unknown>; return typeof fields.type === "string" ? fields.type : null; }',
    ),
    valid('export function f(raw: unknown) { return raw as unknown; }'),
    // A literal written in place has the shape it is written with.
    valid('export const counts = {} as Record<string, number>;'),
    // Containers whose contents are known, or asserted to contents that claim nothing.
    valid('export const frozen = (list: string[]) => list as readonly string[];'),
    valid('export const loose = (raw: unknown[]) => raw as readonly unknown[];'),
    valid('export const later = (raw: Promise<Message>) => raw as Promise<unknown>;'),
    // An empty collection written in place has no outside contents, though its type arguments are `any`.
    valid('export const byId = new Map() as Map<string, Message>;'),
    valid('export const none = [] as Message[];'),
    // Known members stay known through unions and properties.
    valid(
      'export const maybe = (raw: string[] | undefined) => raw as readonly string[] | undefined;',
    ),
    valid(
      'export const wrapped = (raw: { data: Message }) => raw as { data: Message; extra?: string };',
    ),
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
    // Knowing a value is an object says nothing about its shape.
    invalid(
      'export function f(raw: unknown) { if (typeof raw === "object" && raw !== null) return raw as Message; return null; }',
    ),
    invalid('export function f(raw: object) { return raw as Message; }'),
    invalid('export function f(raw: {}) { return raw as Message; }'),
    invalid('export function f(raw: Record<string, unknown>) { return raw as Message; }'),
    invalid('export function f(raw: Record<string, any>) { return <Message>raw; }'),
    invalid('export function f(raw: object | null) { return raw!; }'),
    invalid('export function f(raw: string | object) { return raw as Message; }'),
    // A type parameter is as unchecked as its constraint, and one without a constraint may be anything.
    invalid('export function f<T>(raw: T) { return raw as Message; }'),
    invalid('export function f<T extends object>(raw: T) { return raw as Message; }'),
    invalid(
      'export function f<T extends Record<string, unknown>>(raw: T) { return raw as Message; }',
    ),
    // Going through `unknown` does not launder the claim.
    invalid('export function f(raw: object) { return raw as unknown as Message; }'),
    // A type parameter as the target claims whatever the caller picks.
    invalid('export function cast<T>(raw: unknown): T { return raw as T; }'),
    invalid('export function cast<T extends object>(raw: object): T { return raw as T; }'),
    // Unchecked values inside a container claim just as much.
    invalid(
      'export function f(raw: unknown) { return Array.isArray(raw) ? raw as Message[] : []; }',
    ),
    invalid('export function f(raw: unknown[]) { return raw as Message[]; }'),
    invalid('export function f(raw: Promise<unknown>) { return raw as Promise<Message>; }'),
    invalid('export function f(raw: Map<string, unknown>) { return raw as Map<string, Message>; }'),
    invalid('export function f(raw: Promise<unknown[]>) { return raw as Promise<Message[]>; }'),
    // Member by member of a union.
    invalid(
      'export function f(raw: unknown[] | undefined) { return raw as Message[] | undefined; }',
    ),
    invalid(
      'export function f(raw: Promise<unknown> | null) { return raw as Promise<Message> | null; }',
    ),
    // An array into a tuple.
    invalid('export function f(raw: unknown[]) { return raw as [Message, Message]; }'),
    // An unknown property into a known one.
    invalid('export function f(raw: { data: unknown }) { return raw as { data: Message }; }'),
    invalid(
      "export function f(raw: Record<'a' | 'b', unknown>) { return raw as { a: Message; b: string }; }",
    ),
    // An array with elements or a constructor with arguments carries what it is given.
    invalid('export function f(raw: unknown[]) { return [...raw] as Message[]; }'),
    invalid('export function f(raw: unknown) { return [raw] as [Message]; }'),
    invalid('export function f(raw: unknown[]) { return new Set(raw) as Set<Message>; }'),
  ],
});
