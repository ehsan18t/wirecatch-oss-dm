// What the mock core does with settings: change them, and export and import settings files.

import {
  PROFILE_FORMAT_VERSION,
  type AppSettingsPatch,
  type ImportedFolder,
  type ImportPreview,
  type SettingsProfile,
} from '@wirecatch/protocol';
import { version } from '../../../../package.json';
import { applyPatch } from '../../state/settings-changes.svelte';
import { BackendError } from '../types';
import type { MockState } from './state';

export function changeSettings(state: MockState, patch: AppSettingsPatch): void {
  state.settings = applyPatch(state.settings, patch);
  state.settingsChanged();
}

export function exportSettings(state: MockState, path: string): void {
  state.files.set(path, {
    format_version: PROFILE_FORMAT_VERSION,
    app_version: version,
    exported_at: state.now(),
    settings: structuredClone(state.settings),
    queues: structuredClone(state.queues),
    addons: state.addons.filter((a) => a.state.type !== 'not_installed').map((a) => a.id),
  });
}

function profileAt(state: MockState, path: string): SettingsProfile {
  const profile = state.files.get(path);
  if (!profile) {
    throw new BackendError(
      'That file is not a Wirecatch settings file. Choose a file made with Export settings.',
    );
  }
  return profile;
}

/**
 * The default folder of `profile` as the core would place it: a startup folder is refused and
 * replaced by Downloads, a folder outside the person's own folders is flagged.
 */
function importedFolder(profile: SettingsProfile): ImportedFolder {
  const requested = profile.settings.default_folder;
  const startup = /Start Menu|[\\/]\.config[\\/]autostart/i.test(requested);
  const used = startup ? '{Downloads}' : requested;
  return {
    setting: 'Default folder',
    requested,
    used,
    resolved: used,
    reason: startup
      ? `"${requested}" is a system or startup folder, where a downloaded file could run by itself. Folders from a settings file cannot be there; choose another folder.`
      : null,
    outside_user_folders: !used.startsWith('{'),
  };
}

function addonsToOffer(state: MockState, profile: SettingsProfile): string[] {
  // Like the core: offered, never installed by the import itself.
  return profile.addons.filter(
    (id) => state.addons.find((a) => a.id === id)?.state.type === 'not_installed',
  );
}

export function previewImport(state: MockState, path: string): ImportPreview {
  const profile = profileAt(state, path);
  const known = new Set(state.queues.map((q) => q.id));
  return {
    folders: [importedFolder(profile)],
    queues_added: profile.queues.filter((q) => !known.has(q.id)).map((q) => q.name),
    queues_updated: profile.queues.filter((q) => known.has(q.id)).map((q) => q.name),
    addons_to_offer: addonsToOffer(state, profile),
  };
}

export function importSettings(state: MockState, path: string): void {
  const profile = profileAt(state, path);
  const known = new Set(state.queues.map((q) => q.id));
  const added = profile.queues.filter((q) => !known.has(q.id)).length;
  const folder = importedFolder(profile);
  state.settings = structuredClone(profile.settings);
  state.settings.default_folder = folder.used;
  state.queues = structuredClone(profile.queues);
  state.settingsChanged();
  state.queuesChanged();
  state.emit({
    type: 'settings_imported',
    data: {
      folders: [folder],
      queues_added: added,
      queues_updated: profile.queues.length - added,
      addons_to_offer: addonsToOffer(state, profile),
    },
  });
}
