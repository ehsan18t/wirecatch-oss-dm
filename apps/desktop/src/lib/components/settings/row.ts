// What the settings sections share: what each setting's control is given, and what each section is given.

import type { AppSettings } from '@wirecatch/protocol';
import { createContext } from 'svelte';
import type { FieldControl } from '@wirecatch/ui';
import type { SettingDef, SettingId } from '$lib/settings-schema';

/** Changes settings, saving only what changed. */
export type SaveSettings = (change: (draft: AppSettings) => void) => void;

/** What a setting's control is given: the setting, the settings now, and what labels and describes the control (spread it on the control, as with Field). */
export interface Row {
  def: SettingDef;
  s: AppSettings;
  control: FieldControl;
}

/** What a section is given. */
export interface SectionProps {
  /** The section's settings that match the search, in order. */
  items: readonly SettingDef[];
  settings: AppSettings;
  /** The setting just opened from elsewhere, highlighted so the eye finds it. */
  flash: SettingId | null;
  /** Changes settings, saving only what changed. */
  save: SaveSettings;
}

const [searchingContext, setSearchingContext, hasSearchingContext] = createContext<() => boolean>();

/** Tells the sections below whether search results show, which puts each under a heading of its section's name. */
export function setSettingsSearching(searching: () => boolean): void {
  setSearchingContext(searching);
}

/** Whether search results show; a section shown on its own (tests, the gallery) is not one. */
export function settingsSearching(): () => boolean {
  return hasSearchingContext() ? searchingContext() : () => false;
}
