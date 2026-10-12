import { after, describe, it } from 'node:test';
import { RuleTester } from 'eslint';
import { noValueOfType } from './no-value-of-type.js';
import { filename, typedRuleTester } from './typed-rule-tester.js';

RuleTester.describe = describe;
RuleTester.it = it;
RuleTester.afterAll = after;

const tester = typedRuleTester();
const DECLARATIONS = `
interface Backend { pickFolder(): Promise<string | null> }
class MockBackend implements Backend { pickFolder() { return Promise.resolve(null); } }
interface App { backend: Backend; ready: boolean; folder: string }
declare const app: App;
declare function getApp(): App;
declare const key: keyof App;
`;
const options = [{ types: ['Backend', 'MockBackend'], message: 'Ask the state.' }];
const valid = (code) => ({ code: DECLARATIONS + code, filename, options });
const invalid = (code, count = 1) => ({
  code: DECLARATIONS + code,
  filename,
  options,
  errors: Array.from({ length: count }, () => ({ messageId: 'banned' })),
});

tester.run('no-value-of-type', noValueOfType, {
  valid: [
    valid('export const ready = app.ready;'),
    valid('export const s = app; export const folder = s.folder;'),
    valid(
      'export async function pick(state: { pick(): Promise<string | null> }) { return state.pick(); }',
    ),
    valid('export const { ready } = app;'),
  ],
  invalid: [
    invalid('export const picked = app.backend.pickFolder();'),
    // Through another name, a call or a computed key: the type is what counts.
    invalid('const s = app; export const picked = s.backend.pickFolder();'),
    invalid('export const picked = getApp().backend.pickFolder();'),
    invalid('export const value = app[key];'),
    invalid('export const value = app["backend"];'),
    // Destructuring and parameters.
    invalid('export const { backend } = app;'),
    invalid('export function f({ backend }: { backend: Backend }) { return backend; }'),
    invalid('export function f(backend: Backend) { return backend; }'),
    // A class that implements the interface is named too, and a value that may be one of several.
    invalid('export function f(b: MockBackend) { return b; }'),
    invalid('declare const either: Backend | null; export const b = either;'),
    // A name, an await, an as expression, and the type arguments of generics that hold the value.
    invalid('declare const held: Backend; export const list = [held];'),
    invalid(
      'declare const later: Promise<Backend>; export async function f() { return await later; }',
    ),
    invalid(
      'declare function load(): Promise<Backend>; export async function f() { return await load(); }',
    ),
    invalid('declare const anything: unknown; export const b = anything as Backend;'),
    invalid(
      'interface Readable<T> { subscribe(run: (value: T) => void): () => void }\ndeclare const store: Readable<Backend>; export const s = store;',
    ),
    invalid('export function f(b: Pick<Backend, "pickFolder">) { return b; }'),
    invalid('export function f(b: Partial<Backend>) { return b; }'),
    // Reported once, at the value, not again at each use of its name.
    invalid('const b = app.backend; export const c = b; export const d = b;'),
    // Reflect.get on an object that holds one.
    invalid('export const value = Reflect.get(app, "backend");'),
  ],
});
