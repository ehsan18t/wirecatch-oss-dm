// What the mock core does with add-ons: install, update, stop and remove them, and check for updates.

import type { AddonInfo } from '@wirecatch/protocol';
import { BackendError } from '../types';
import type { MockState } from './state';

/** What the next update check finds; tests and sample data set it. */
export type UpdateCheck = 'up-to-date' | 'newer' | 'too-new' | 'unreachable';

/** The version a check that finds a newer media add-on reports. */
const NEWER_MEDIA_VERSION = 'yt-dlp 2026.10.30, ffmpeg 9.0.2, Deno 2.9.7';

/** Starts installing add-on `id`; its install moves along with the simulation. */
export function installAddon(state: MockState, id: string): void {
  const addon = state.addon(id);
  if (addon.state.type === 'installing') return;
  state.beforeInstall.set(id, structuredClone(addon.state));
  addon.state = { type: 'installing', data: { done: 0, total: addon.size } };
  state.addonsChanged();
}

export function checkAddonUpdates(state: MockState, finds: UpdateCheck): AddonInfo[] {
  const addon = state.addon('media');
  const installed = state.mediaReady();
  if (installed && finds === 'unreachable') {
    throw new BackendError(
      'Wirecatch could not check for a newer media add-on (no connection). Check your internet connection, then try again.',
    );
  }
  // As the core says it (D21.1): a release younger than two days waits.
  addon.note =
    installed && finds === 'too-new'
      ? 'A newer version was released today; Wirecatch installs it after two days.'
      : null;
  state.addonsChanged();
  if (installed && finds === 'newer') {
    addon.state = { type: 'update_available' };
    addon.latest_version = NEWER_MEDIA_VERSION;
    state.addonsChanged();
  }
  return structuredClone(state.addons);
}

export function cancelAddonOperation(state: MockState, id: string): void {
  const addon = state.addon(id);
  if (addon.state.type !== 'installing') return;
  // Like the core: an update keeps the installed version; a first install leaves nothing.
  const before = state.beforeInstall.get(id);
  if (before?.type === 'installed' || before?.type === 'update_available') {
    addon.state = before;
  } else {
    addon.state = { type: 'not_installed' };
    addon.installed_version = null;
  }
  state.addonsChanged();
}

export function removeAddon(state: MockState, id: string): void {
  const addon = state.addon(id);
  addon.state = { type: 'not_installed' };
  addon.installed_version = null;
  state.addonsChanged();
}
