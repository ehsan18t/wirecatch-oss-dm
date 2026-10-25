// Reading and writing a queue setting that follows the app or has the queue's own value (D72.1).
import type { Setting } from './generated/Setting';

export const FOLLOW_APP = { source: 'follow_app' } as const satisfies Setting<never>;

export function ownSetting<T>(value: T): Setting<T> {
  return { source: 'own', value };
}

/** The queue's own value, or `null` when it follows the app. */
export function ownValue<T>(setting: Setting<T>): T | null {
  return setting.source === 'own' ? setting.value : null;
}
