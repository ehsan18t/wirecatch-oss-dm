import type { StorageArea } from './cache';

/** Key of this browser profile's id in `storage.local`. */
export const PROFILE_KEY = 'wirecatch.profile.v1';

/**
 * This browser profile's id for page reads (D282), made once and kept: the app sends a download's page reads only to the profile it came from, since two profiles hold different sign-ins.
 */
export async function browserProfile(area: StorageArea, newId: () => string): Promise<string> {
  const kept = (await area.get(PROFILE_KEY))[PROFILE_KEY];
  if (typeof kept === 'string' && kept !== '') return kept;
  const id = newId();
  await area.set({ [PROFILE_KEY]: id });
  return id;
}
