import { after, describe, it } from 'node:test';
import { RuleTester } from 'eslint';
import { noTypeArgumentAssertion } from './no-type-argument-assertion.js';
import { filename, typedRuleTester } from './typed-rule-tester.js';

RuleTester.describe = describe;
RuleTester.it = it;
RuleTester.afterAll = after;

const tester = typedRuleTester();
const DECLARATIONS = `
type Settings = { on: boolean };
type AppEvent<T> = { name: string; payload: T };
declare function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
declare function listen<T>(name: string, handler: (event: AppEvent<T>) => void): Promise<() => void>;
declare function clone<T>(value: T): T;
declare function first<T>(items: readonly T[]): T | undefined;
declare function build<T>(make: () => T): T;
declare function pick<K extends string, T>(key: K, from: { [P in K]: T }): T;
type EventCallback<T> = (event: AppEvent<T>) => void;
declare function on<T>(name: string, handler: EventCallback<T>): void;
type Guard<T> = (value: unknown) => value is T;
declare function fields<T>(shape: { [K in keyof T]-?: Guard<T[K]> }): Guard<T>;
declare const isBool: Guard<boolean>;
declare function $state<T>(): T | undefined;
declare function load<T>(): T;
declare function onMessage(handler: (message: any) => void): void;
declare function onNamed<K extends string>(name: K, handler: (event: AppEvent<K>) => void): void;
`;
const valid = (code) => ({ code: DECLARATIONS + code, filename });
const invalid = (code, messageId = 'assertion') => ({
  code: DECLARATIONS + code,
  filename,
  errors: [{ messageId }],
});

tester.run('no-type-argument-assertion', noTypeArgumentAssertion, {
  valid: [
    valid("export const s = invoke('get_settings');"),
    valid('export const s = clone<Settings>({ on: true });'),
    valid('export const s = first<Settings>([{ on: true }]);'),
    valid('export const s = build<Settings>(() => ({ on: true }));'),
    valid("export const s = pick<'a', number>('a', { a: 1 });"),
    valid('export const m = new Map<string, number>();'),
    // Given nothing, it has nothing to check: the type argument declares what will be put there.
    valid('export const el = $state<HTMLDivElement>();'),
    valid('onMessage((message: unknown) => message);'),
    valid("onNamed('a', (event: AppEvent<'a'>) => event.name);"),
    valid('[1, 2].forEach((n: number) => n);'),
    valid("listen('settings', (e) => e.payload);"),
    valid('export const isSettings = fields<Settings>({ on: isBool });'),
    valid('export const p = new Promise<number>((resolve) => { resolve(1); });'),
    valid('export const n = [1, 2].map<string>((v) => String(v));'),
  ],
  invalid: [
    invalid("export const s = invoke<Settings>('get_settings');"),
    invalid("export const stop = listen<Settings>('settings', (e) => e.payload.on);"),
    invalid("on<Settings>('settings', (e) => e.payload.on);"),
    // Given nothing, a call asserts what it returns unless it only declares.
    invalid('export const s = load<Settings>();'),
    // The annotation says what the type parameter is, as a type argument would.
    invalid("listen('settings', (e: AppEvent<Settings>) => e.payload.on);", 'annotation'),
    invalid('onMessage((message: Settings) => message.on);', 'anyParameter'),
    invalid("export const input = document.querySelector<HTMLInputElement>('input');"),
  ],
});
