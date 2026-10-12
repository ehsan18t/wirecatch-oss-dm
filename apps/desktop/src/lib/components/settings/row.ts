// What the settings sections share: what each setting's control is given, and what each
// section is given.

import type { AppSettings } from '@wirecatch/protocol';
import type { SettingDef, SettingId } from '$lib/settings-schema';

/** What a setting's control is given: the setting, the settings now, and its help text's id. */
export interface Row {
  def: SettingDef;
  s: AppSettings;
  help: string;
}

/** What a section is given. */
export interface SectionProps {
  /** The section's settings that match the search, in order. */
  items: readonly SettingDef[];
  settings: AppSettings;
  /** The setting just opened from elsewhere, highlighted so the eye finds it. */
  flash: SettingId | null;
}
