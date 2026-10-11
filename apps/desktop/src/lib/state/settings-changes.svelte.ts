// Settings changes as a patch of the settings that changed, never the whole document. Each window holds its own copy of the settings, and a copy can be behind a change made in another window or in the extension; sending only what changed means a window never undoes what it did not touch.

import type {
  AppSettings,
  AppSettingsPatch,
  ExtensionSettings,
  ExtensionSettingsPatch,
  MediaDefaults,
  MediaDefaultsPatch,
} from '@wirecatch/protocol';

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function same(a: unknown, b: unknown): boolean {
  if (a === b) return true;
  if (Array.isArray(a) && Array.isArray(b)) {
    return a.length === b.length && a.every((item, index) => same(item, b[index]));
  }
  if (!isRecord(a) || !isRecord(b)) return false;
  const keys = Object.keys(a);
  return keys.length === Object.keys(b).length && keys.every((key) => same(a[key], b[key]));
}

/** The fields of `after` that differ from `before`, each whole. */
function changedFields<T extends object>(before: T, after: T): Partial<T> {
  const changed: Partial<T> = {};
  for (const key of Object.keys(after) as (keyof T)[]) {
    if (!same(before[key], after[key])) changed[key] = after[key];
  }
  return changed;
}

function mediaPatch(before: MediaDefaults, after: MediaDefaults): MediaDefaultsPatch {
  const patch: MediaDefaultsPatch = {};
  if (!same(before.video, after.video)) patch.video = changedFields(before.video, after.video);
  if (!same(before.audio, after.audio)) patch.audio = changedFields(before.audio, after.audio);
  return patch;
}

/**
 * The patch that turns `before` into `after`, or null when nothing changed. Every setting is replaced whole, a list included; the groups (`extension`, `media` and its two parts) hold only their own settings that changed.
 */
export function settingsPatch(before: AppSettings, after: AppSettings): AppSettingsPatch | null {
  const patch: AppSettingsPatch = changedFields(before, after);
  if (patch.extension !== undefined) {
    patch.extension = changedFields(before.extension, after.extension);
  }
  if (patch.media !== undefined) patch.media = mediaPatch(before.media, after.media);
  return Object.keys(patch).length === 0 ? null : patch;
}

/** Puts back, from `before`, each field `patch` set that still holds what it set in `current`. */
function restoreFields<T extends object>(
  current: T,
  patch: { [K in keyof T]?: unknown },
  before: T,
): void {
  for (const key of Object.keys(patch) as (keyof T)[]) {
    if (same(current[key], patch[key])) current[key] = structuredClone(before[key]);
  }
}

/**
 * `current` with a failed `patch` taken back to the values in `before`, each setting only if it still holds what the patch set: a newer change to the same setting, from this window or another, is kept.
 */
export function withoutPatch(
  current: AppSettings,
  patch: AppSettingsPatch,
  before: AppSettings,
): AppSettings {
  const restored = structuredClone(current);
  const { extension, media, ...rest } = patch;
  restoreFields(restored, rest, before);
  if (extension !== undefined) restoreFields(restored.extension, extension, before.extension);
  if (media?.video !== undefined) {
    restoreFields(restored.media.video, media.video, before.media.video);
  }
  if (media?.audio !== undefined) {
    restoreFields(restored.media.audio, media.audio, before.media.audio);
  }
  return restored;
}

/** Sends settings patches one after another, so the app applies a window's changes in the order the person made them. */
export class SettingsWriter {
  #last: Promise<unknown> = Promise.resolve();
  readonly #send: (patch: AppSettingsPatch) => Promise<boolean>;

  /** `send` delivers one patch and says whether the app took it. */
  constructor(send: (patch: AppSettingsPatch) => Promise<boolean>) {
    this.#send = send;
  }

  /** Queues a patch after the ones before it; resolves with whether the app took it. */
  write(patch: AppSettingsPatch): Promise<boolean> {
    const result = this.#last.then(() => this.#send(patch));
    this.#last = result.catch(() => undefined);
    return result;
  }
}

/** Anything that shows a copy of the settings: the main window and each download window. */
export interface SettingsHolder {
  settings: AppSettings | null;
}

/**
 * Changes the copy in `holder` at once, so the screen answers right away, and sends only what `change` changed. If the app refuses, only those settings are taken back.
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
  const patch = settingsPatch(before, draft);
  if (patch === null) return;
  holder.settings = draft;
  const taken = await writer.write(patch);
  if (!taken) holder.settings = withoutPatch($state.snapshot(holder.settings), patch, before);
}

/** Sets each field the patch has a value for; `null` is a value (it clears a setting that can be empty). */
function assignFields<T extends object>(target: T, patch: Partial<T>): void {
  for (const key of Object.keys(patch) as (keyof T)[]) {
    const value = patch[key];
    if (value !== undefined) target[key] = structuredClone(value);
  }
}

/** The extension's settings with a patch applied the way the app applies it (`ExtensionSettings::apply`): the whole list of ignored sites first, then additions, then removals; \`null\` leaves a setting as it is. */
function applyExtensionPatch(settings: ExtensionSettings, patch: ExtensionSettingsPatch): void {
  let sites = patch.ignored_sites ?? settings.ignored_sites;
  for (const host of patch.add_ignored_sites ?? []) {
    if (!sites.includes(host)) sites = [...sites, host];
  }
  const removed = patch.remove_ignored_sites ?? [];
  settings.ignored_sites = sites.filter((host) => !removed.includes(host));
  settings.capture_enabled = patch.capture_enabled ?? settings.capture_enabled;
  settings.ask_before_download = patch.ask_before_download ?? settings.ask_before_download;
  settings.show_media_button = patch.show_media_button ?? settings.show_media_button;
  settings.capture_extensions = patch.capture_extensions ?? settings.capture_extensions;
  settings.min_capture_size = patch.min_capture_size ?? settings.min_capture_size;
  settings.default_queue = patch.default_queue ?? settings.default_queue;
}

/** `settings` with `patch` applied the way the app applies it (`AppSettings::apply`). Used by the stand-in backend. */
export function applyPatch(settings: AppSettings, patch: AppSettingsPatch): AppSettings {
  const next = structuredClone(settings);
  const { extension, media, ...rest } = patch;
  assignFields(next, rest);
  if (extension !== undefined) applyExtensionPatch(next.extension, extension);
  if (media?.video !== undefined) assignFields(next.media.video, media.video);
  if (media?.audio !== undefined) assignFields(next.media.audio, media.audio);
  return next;
}
