// Searching the settings by the words a person types.

import { SETTINGS, type SettingDef } from './settings-schema';

/** Settings whose label, explanation or keywords contain every word typed. */
export function searchSettings(query: string): SettingDef[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) return [...SETTINGS];
  return SETTINGS.filter((s) => {
    const hay = `${s.label} ${s.help} ${s.keywords}`.toLowerCase();
    return words.every((w) => hay.includes(w));
  });
}
