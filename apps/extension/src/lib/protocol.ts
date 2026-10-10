import type { AddAction, ExtensionSettings, ExtensionSettingsPatch } from '@wirecatch/protocol';

/**
 * Must match `wirecatch_protocol::PROTOCOL_VERSION`. The generated TypeScript only
 * carries types, so the constant is repeated here; `hello` fails loudly on a mismatch.
 */
export const PROTOCOL_VERSION = 1;

/** A patch that changes only the given fields; every other field is sent as null (no change). */
export function patchOf(changes: Partial<ExtensionSettings>): ExtensionSettingsPatch {
  return {
    capture_enabled: changes.capture_enabled ?? null,
    ask_before_download: changes.ask_before_download ?? null,
    show_media_button: changes.show_media_button ?? null,
    ignored_sites: changes.ignored_sites ?? null,
    capture_extensions: changes.capture_extensions ?? null,
    min_capture_size: changes.min_capture_size ?? null,
    default_queue: changes.default_queue ?? null,
  };
}

/** Applies a patch the same way the app does (`ExtensionSettings::apply`). */
export function applyPatch(settings: ExtensionSettings, patch: ExtensionSettingsPatch): ExtensionSettings {
  return {
    capture_enabled: patch.capture_enabled ?? settings.capture_enabled,
    ask_before_download: patch.ask_before_download ?? settings.ask_before_download,
    show_media_button: patch.show_media_button ?? settings.show_media_button,
    ignored_sites: patch.ignored_sites ?? settings.ignored_sites,
    capture_extensions: patch.capture_extensions ?? settings.capture_extensions,
    min_capture_size: patch.min_capture_size ?? settings.min_capture_size,
    default_queue: patch.default_queue ?? settings.default_queue,
  };
}

/**
 * True when the app now owns the download, so the browser copy may go. Anything
 * else (including a value a newer app might invent) keeps the browser copy.
 */
export function isConfirmed(action: AddAction): boolean {
  return CONFIRMED_ACTIONS.has(action);
}

const CONFIRMED_ACTIONS: ReadonlySet<string> = new Set<AddAction>([
  'started',
  'queued',
  'awaiting_confirmation',
]);
