// Add-ons in plain words: what state one is in and what the person can do about it.

import type { AddonInfo, AddonState } from '@wirecatch/protocol';
import { formatBytes } from './format';

/** The id of the one add-on (D20): yt-dlp, ffmpeg and Deno, installed together. */
export const MEDIA_ADDON = 'media';

/** How far an install has got, from 0 to 1, or `null` when the size is not known. */
export function installFraction(state: AddonState): number | null {
  if (state.type !== 'installing') return null;
  const { done, total } = state.data;
  if (total === null || total <= 0) return null;
  return Math.min(1, done / total);
}

/** True when the add-on can be used right now. */
export function isInstalled(addon: AddonInfo | undefined): boolean {
  return addon?.state.type === 'installed' || addon?.state.type === 'update_available';
}

/** One line on where the add-on stands. */
export function addonStatus(addon: AddonInfo): string {
  const state = addon.state;
  switch (state.type) {
    case 'not_installed':
      return addon.size === null
        ? 'Not installed.'
        : `Not installed. Download size ${formatBytes(addon.size)}.`;
    case 'installing': {
      const fraction = installFraction(state);
      return fraction === null ? 'Installing.' : `Installing, ${Math.floor(fraction * 100)}%.`;
    }
    case 'installed':
      return addon.installed_version
        ? `Installed, version ${addon.installed_version}.`
        : 'Installed.';
    case 'update_available':
      return addon.latest_version
        ? `Version ${addon.latest_version} is ready to install.`
        : 'A newer version is ready to install.';
    case 'failed':
      return `Could not install: ${state.data.message}`;
  }
}

/** The main button for the add-on's state, or `null` while it installs. */
export function addonAction(addon: AddonInfo): 'install' | 'update' | 'retry' | null {
  switch (addon.state.type) {
    case 'not_installed':
      return 'install';
    case 'update_available':
      return 'update';
    case 'failed':
      return 'retry';
    case 'installing':
    case 'installed':
      return null;
  }
}
