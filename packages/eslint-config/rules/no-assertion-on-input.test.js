import { after, describe, it } from 'node:test';
import { RuleTester } from 'eslint';
import ts from 'typescript-eslint';
import { noAssertionOnInput } from './no-assertion-on-input.js';

RuleTester.describe = describe;
RuleTester.it = it;
RuleTester.afterAll = after;

const tester = new RuleTester({ languageOptions: { parser: ts.parser } });
const assertion = [{ messageId: 'assertion' }];

tester.run('no-assertion-on-input', noAssertionOnInput, {
  valid: [
    // Checked with a guard, not asserted.
    'function asRequest(raw: unknown): Request | null { return isRequest(raw) ? raw : null; }',
    // `as const` says nothing about the input.
    'function kind(raw: unknown) { const KINDS = ["a", "b"] as const; return KINDS.find((k) => k === raw) ?? null; }',
    'function kind(raw: unknown) { return <const>["a", "b"]; }',
    // Code that does not read outside data may still narrow a browser type it knows.
    'const tab = info as MenuTab;',
    'function onPointer(e: Event) { return (e as PointerEvent).relatedTarget; }',
    // A parameter that only contains `unknown` is a typed value, not raw input.
    'function changed(changes: Record<string, { newValue?: unknown }>) { return changes.a as Change; }',
  ],
  invalid: [
    { code: 'function asRequest(raw: unknown) { return raw as Request; }', errors: assertion },
    { code: 'const read = (stored: unknown) => (stored as { on?: boolean }).on === true;', errors: assertion },
    { code: 'function asRequest(raw: unknown) { return <Request>raw; }', errors: assertion },
    // A type guard is where data gets its type too.
    { code: 'function isMessage(raw): raw is Message { return (raw as Message).type === "x"; }', errors: assertion },
    // Inside a callback of such a function.
    {
      code: 'function restore(saved: unknown) { return [saved].map((item) => item as Entry); }',
      errors: assertion,
    },
    // Defaults and rest parameters count.
    { code: 'function read(raw: unknown = null) { return raw as string; }', errors: assertion },
    { code: 'function first(...raw: unknown) { return raw as string[]; }', errors: assertion },
    // A method of a class that takes a message.
    { code: 'class C { onMessage(message: unknown) { return message as Reply; } }', errors: assertion },
  ],
});
