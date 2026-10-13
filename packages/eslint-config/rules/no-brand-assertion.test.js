import { after, describe, it } from 'node:test';
import { RuleTester } from 'eslint';
import { noBrandAssertion } from './no-brand-assertion.js';
import { filename, typedRuleTester } from './typed-rule-tester.js';

RuleTester.describe = describe;
RuleTester.it = it;
RuleTester.afterAll = after;

const tester = typedRuleTester();
const DECLARATIONS = `
type QueueId = number & { readonly __brand: "QueueId" };
type DownloadId = number & { readonly __brand: "DownloadId" };
type Meters = number;
declare function queueId(value: number): QueueId;
declare const raw: number;
declare function isQueueId(value: unknown): value is QueueId;
`;
/** The constructor module is named by the end of its path; the fixture file stands in for it. */
const constructorsHere = [{ constructors: ['rules/fixtures/file.ts'] }];
const elsewhere = [{ constructors: ['packages/protocol/src/ids.ts'] }];
const valid = (code, options = elsewhere) => ({ code: DECLARATIONS + code, filename, options });
/** The fixture file stands in for the generated types too. */
const generatedHere = [{ ...elsewhere[0], generated: ['rules/fixtures/'] }];
const invalid = (code, messageId = 'brand', options = elsewhere) => ({
  code: DECLARATIONS + code,
  filename,
  options,
  errors: [{ messageId }],
});

tester.run('no-brand-assertion', noBrandAssertion, {
  valid: [
    valid('export const q = queueId(3);'),
    valid('export const m = raw as Meters;'),
    valid('export const n = 3 as const;'),
    // `as const` keeps the types of what is written, ids made by their constructor included.
    valid("export const views = [{ kind: 'queue', id: queueId(2) }] as const;"),
    valid('declare const q: QueueId; export const n = q as number;'),
    // The constructor module is where a number becomes an id.
    valid(
      'export function make(value: number): QueueId { return value as QueueId; }',
      constructorsHere,
    ),
    valid('export const fake = (v: unknown): v is QueueId => true;', constructorsHere),
    valid('export const q = isQueueId(raw);'),
    valid('declare const q: QueueId; export const same = isQueueId(q);'),
    // A guard for a type of the program's own, not one the protocol generates.
    valid(
      'type Local = { a: number }; export const isLocal = (v: unknown): v is Local => v !== null;',
    ),
  ],
  invalid: [
    invalid('export const q = 3 as QueueId;'),
    invalid('export const q = <QueueId>raw;'),
    invalid('export const q = raw as QueueId | null;'),
    invalid('export const ids = [1, 2] as DownloadId[];'),
    invalid('export const ids = [raw] as readonly DownloadId[];'),
    // One id passed off as another.
    invalid('declare const q: QueueId; export const d = q as unknown as DownloadId;'),
    invalid('export const row = { id: raw } as { id: DownloadId };'),
    // Hand-written guards claim what the generated ones check.
    invalid('export const fake = (v: unknown): v is QueueId => true;', 'predicate'),
    invalid('export function fake(v: unknown): v is QueueId | null { return true; }', 'predicate'),
    invalid(
      'type Queue = { id: QueueId; name: string }; export const isQueue = (v: unknown): v is Queue => v !== null;',
      'predicate',
      generatedHere,
    ),
    invalid(
      'type Settings = { on: boolean }; export const isSettings = (v: unknown): v is Settings | null => v === null;',
      'predicate',
      generatedHere,
    ),
    invalid(
      'type Settings = { on: boolean }; export const areSettings = (v: unknown): v is Settings[] => Array.isArray(v);',
      'predicate',
      generatedHere,
    ),
    // A guard narrows what it is given, another id included.
    invalid('declare const d: DownloadId; export const both = isQueueId(d);', 'otherId'),
  ],
});
