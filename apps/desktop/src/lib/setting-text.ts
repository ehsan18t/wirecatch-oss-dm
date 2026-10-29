// What each setting says about itself, for the list of every setting (settings-schema.ts).

import { SETTING_WORDS } from '@wirecatch/protocol';
import type { SectionId } from './settings-sections';

/** The name and help of a setting the extension's popup shows too, in the words both share (D61). */
export function words(id: keyof typeof SETTING_WORDS): { label: string; help: string } {
  const { label, help } = SETTING_WORDS[id];
  return { label, help };
}

/** What every setting says about itself. */
export interface SettingText {
  id: string;
  section: SectionId;
  label: string;
  help: string;
  /** Extra words people might search for. */
  keywords: string;
  /** The heading the setting sits under within its section. */
  group?: string;
}
