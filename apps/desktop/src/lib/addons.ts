// Add-ons in plain words: what state one is in and what the person can do about it.

import type { AddonId, AddonInfo, AddonState } from '@wirecatch/protocol';
import { BackendError } from './backend';
import { PERCENT_MAX, percentOf, roundDownTo } from './format';

/** A screen reader hears an install at every quarter of it, in percent, not at every percent. */
const ANNOUNCE_STEP = 25;

/** The id of the one add-on (D20): yt-dlp, ffmpeg and Deno, installed together. */
export const MEDIA_ADDON: AddonId = 'media';

/**
 * Whether an error is the core's "this needs the media add-on" (`CoreError::AddonRequired`),
 * which comes with an `addon_required` prompt, rather than any other failure. Told by its code.
 */
export function isAddonRequired(err: unknown): boolean {
  return err instanceof BackendError && err.code === 'addon_required';
}

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
      return 'Not installed.';
    case 'installing': {
      const fraction = installFraction(state);
      if (fraction === null) return 'Installing.';
      // Everything is downloaded; the files are being checked and unpacked.
      if (fraction >= 1) return 'Installing: verifying files.';
      return `Installing, ${percentOf(fraction)}%.`;
    }
    case 'installed':
      return addon.installed_version ? `Installed: ${addon.installed_version}.` : 'Installed.';
    case 'update_available':
      return addon.latest_version
        ? `Version ${addon.latest_version} is ready to install.`
        : 'A newer version is ready to install.';
    case 'failed':
      // The core's message already says what failed and why.
      return state.data.message;
  }
}

/**
 * What a screen reader hears about the add-on. It changes only at the start of an install, at
 * every quarter, and at the end, so a live region reading it does not call out every percent.
 */
export function addonAnnouncement(addon: AddonInfo): string {
  const state = addon.state;
  switch (state.type) {
    case 'not_installed':
    case 'update_available':
      return '';
    case 'installing': {
      const fraction = installFraction(state);
      const quarter = fraction === null ? 0 : roundDownTo(fraction * PERCENT_MAX, ANNOUNCE_STEP);
      return quarter === 0 || quarter === PERCENT_MAX
        ? `Installing the ${addon.name.toLowerCase()}.`
        : `${addon.name}: ${quarter}% installed.`;
    }
    case 'installed':
      return `${addon.name} installed.`;
    case 'failed':
      return addonStatus(addon);
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
