import type { ExtensionSettings, ExtensionSettingsPatch } from '@wirecatch/protocol';

/** The fields a popup change can carry: whole settings fields plus the list edits. */
export type SettingsChange = Partial<ExtensionSettings> & {
  add_ignored_sites?: string[];
  remove_ignored_sites?: string[];
};

/** A patch that changes only the given fields; every other field is sent as null (no change). */
export function patchOf(changes: SettingsChange): ExtensionSettingsPatch {
  return {
    capture_enabled: changes.capture_enabled ?? null,
    ask_before_download: changes.ask_before_download ?? null,
    show_media_button: changes.show_media_button ?? null,
    ignored_sites: changes.ignored_sites ?? null,
    add_ignored_sites: changes.add_ignored_sites ?? null,
    remove_ignored_sites: changes.remove_ignored_sites ?? null,
    capture_extensions: changes.capture_extensions ?? null,
    min_capture_size: changes.min_capture_size ?? null,
    default_queue: changes.default_queue ?? null,
  };
}
