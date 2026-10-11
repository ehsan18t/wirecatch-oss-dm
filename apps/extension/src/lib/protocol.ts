import type { AddAction, ExtensionSettings, ExtensionSettingsPatch } from '@wirecatch/protocol';

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

/**
 * Applies a patch the same way the app does (`ExtensionSettings::apply`): the
 * whole list first, then additions (skipping ones already there), then removals.
 */
export function applyPatch(
  settings: ExtensionSettings,
  patch: ExtensionSettingsPatch,
): ExtensionSettings {
  let sites = patch.ignored_sites ?? settings.ignored_sites;
  for (const host of patch.add_ignored_sites ?? []) {
    if (!sites.includes(host)) sites = [...sites, host];
  }
  const removed = patch.remove_ignored_sites ?? [];
  sites = sites.filter((host) => !removed.includes(host));
  return {
    capture_enabled: patch.capture_enabled ?? settings.capture_enabled,
    ask_before_download: patch.ask_before_download ?? settings.ask_before_download,
    show_media_button: patch.show_media_button ?? settings.show_media_button,
    ignored_sites: sites,
    capture_extensions: patch.capture_extensions ?? settings.capture_extensions,
    min_capture_size: patch.min_capture_size ?? settings.min_capture_size,
    default_queue: patch.default_queue ?? settings.default_queue,
  };
}

const CONFIRMED_ACTIONS: ReadonlySet<string> = new Set<AddAction>([
  'started',
  'queued',
  'awaiting_confirmation',
]);

/**
 * True when the app has accepted the download, so the browser copy may go.
 * Anything else (including a value a newer app might invent) keeps the browser copy.
 *
 * Only `started` means the app has already reached the server and data is flowing.
 * `queued` and `awaiting_confirmation` mean accepted but the link not yet tried, so
 * there is a remaining risk: a link that only works once, or one that needs the
 * browser's POST request, may fail later in the app after the browser copy is gone.
 * The app then shows the error with a retry. The extension's webRequest access only
 * observes responses (to spot media); it does not look at download requests.
 */
export function isConfirmed(action: AddAction): boolean {
  return CONFIRMED_ACTIONS.has(action);
}
