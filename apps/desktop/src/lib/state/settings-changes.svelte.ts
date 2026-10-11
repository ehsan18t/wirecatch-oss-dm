// Settings changes as a list of the settings that changed, never the whole document. Each window holds its own copy of the settings, and a copy can be behind a change made in another window or in the extension; sending only what changed means a window never undoes what it did not touch.

import type { AppSettings, SettingsChange } from '@wirecatch/protocol';

type Json = unknown;

function isGroup(value: Json): value is Record<string, Json> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function same(a: Json, b: Json): boolean {
  if (a === b) return true;
  if (Array.isArray(a) && Array.isArray(b)) {
    return a.length === b.length && a.every((item, index) => same(item, b[index]));
  }
  if (!isGroup(a) || !isGroup(b)) return false;
  const keys = Object.keys(a);
  return keys.length === Object.keys(b).length && keys.every((key) => same(a[key], b[key]));
}

/**
 * The settings that differ between `before` and `after`. Groups present on both sides are compared setting by setting; anything else (a list, a value, a group that appeared or a value whose kind changed) is replaced whole.
 */
export function settingsChanges(before: AppSettings, after: AppSettings): SettingsChange[] {
  const changes: SettingsChange[] = [];
  const walk = (old: Json, next: Json, path: string[]): void => {
    if (isGroup(old) && isGroup(next) && (path.length === 0 || isGroupPath(old, next))) {
      for (const key of Object.keys(next)) walk(old[key], next[key], [...path, key]);
      return;
    }
    if (!same(old, next)) changes.push({ path, value: next });
  };
  walk(before, after, []);
  return changes;
}

/**
 * Whether two values at the same place are groups of settings to compare one by one, rather than one value. A group keeps its keys; a value shaped like an object that changes its keys (a tagged choice, such as a folder that switches from a known folder to a path) is replaced whole.
 */
function isGroupPath(old: Record<string, Json>, next: Record<string, Json>): boolean {
  const oldKeys = Object.keys(old);
  return oldKeys.length === Object.keys(next).length && oldKeys.every((key) => key in next);
}

function valueAt(settings: Json, path: readonly string[]): Json {
  let value = settings;
  for (const key of path) {
    if (!isGroup(value)) return undefined;
    value = value[key];
  }
  return value;
}

function setAt(settings: Json, path: readonly string[], value: Json): void {
  const parents = path.slice(0, -1);
  const last = path.at(-1);
  const group = valueAt(settings, parents);
  if (last !== undefined && isGroup(group)) group[last] = value;
}

/**
 * `current` with the failed `changes` taken back to their values in `before`, each only if it still holds what the change set: a newer change to the same setting, from this window or another, is kept.
 */
export function withoutChanges(
  current: AppSettings,
  changes: readonly SettingsChange[],
  before: AppSettings,
): AppSettings {
  const restored = structuredClone(current);
  for (const change of changes) {
    if (same(valueAt(restored, change.path), change.value)) {
      setAt(restored, change.path, structuredClone(valueAt(before, change.path)));
    }
  }
  return restored;
}

/** Sends settings changes one after another, so the app applies a window's changes in the order the person made them. */
export class SettingsWriter {
  #last: Promise<unknown> = Promise.resolve();
  readonly #send: (changes: SettingsChange[]) => Promise<boolean>;

  /** `send` delivers one batch and says whether the app took it. */
  constructor(send: (changes: SettingsChange[]) => Promise<boolean>) {
    this.#send = send;
  }

  /** Queues a batch after the ones before it; resolves with whether the app took it. */
  write(changes: SettingsChange[]): Promise<boolean> {
    const result = this.#last.then(() => this.#send(changes));
    this.#last = result.catch(() => undefined);
    return result;
  }
}

/** Anything that shows a copy of the settings: the main window and each download window. */
export interface SettingsHolder {
  settings: AppSettings | null;
}

/**
 * Changes the copy in `holder` at once, so the screen answers right away, and sends only what `change` changed. If the app refuses, only those changes are taken back.
 */
export async function changeSettings(
  holder: SettingsHolder,
  change: (draft: AppSettings) => void,
  writer: SettingsWriter,
): Promise<void> {
  if (!holder.settings) return;
  const before = $state.snapshot(holder.settings);
  const draft = structuredClone(before);
  change(draft);
  const changes = settingsChanges(before, draft);
  if (changes.length === 0) return;
  holder.settings = draft;
  const taken = await writer.write(changes);
  if (!taken) holder.settings = withoutChanges($state.snapshot(holder.settings), changes, before);
}

/** `settings` with `changes` applied the way the app applies them: every named setting must already exist. Used by the stand-in backend. */
export function applyChanges(
  settings: AppSettings,
  changes: readonly SettingsChange[],
): AppSettings {
  const next = structuredClone(settings);
  for (const change of changes) {
    const parents = change.path.slice(0, -1);
    const last = change.path.at(-1);
    const group = valueAt(next, parents);
    if (last === undefined || !isGroup(group) || !(last in group)) {
      throw new Error(`There is no setting ${change.path.join('.')}.`);
    }
    group[last] = structuredClone(change.value);
  }
  return next;
}
