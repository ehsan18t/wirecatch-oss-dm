// Reading and writing a queue setting that follows the app or has the queue's own value (D72.1), and what a queue takes (D72.2).
import type { AddDownloads } from './generated/AddDownloads';
import type { QueueSettings } from './generated/QueueSettings';
import type { Setting } from './generated/Setting';

export const GLOBAL = { source: 'global' } as const satisfies Setting<never>;

export const MANUAL_QUEUE = { kind: 'manual' } as const satisfies AddDownloads;

export function ownSetting<T>(value: T): Setting<T> {
  return { source: 'own', value };
}

/** A manual queue named `name` that follows the app in everything, as the core makes one. */
export function newQueueSettings(name: string): QueueSettings {
  return {
    name,
    description: '',
    max_concurrent: GLOBAL,
    speed_limit: GLOBAL,
    connections: GLOBAL,
    proxy: GLOBAL,
    exceptions: GLOBAL,
    add_downloads: MANUAL_QUEUE,
    schedule: null,
    notify: 'each_download',
    folder: GLOBAL,
  };
}

/** The queue's own value, or `null` when it follows the app. */
export function ownValue<T>(setting: Setting<T>): T | null {
  return setting.source === 'own' ? setting.value : null;
}
