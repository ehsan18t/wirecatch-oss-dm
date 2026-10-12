import type { ExtensionSettings, QueueSummary } from '@wirecatch/protocol';
import type { Snapshot } from '../messages';
import type { CachedSettings, HubDeps } from './deps';

/** The part of the popup's snapshot that comes from the settings. */
export type SettingsView = Pick<Snapshot, 'settings' | 'settingsFromCache' | 'cachedAt' | 'queues'>;

/**
 * The app's extension settings and queues as the background knows them: live from a
 * connected app, and the copy cached in browser storage (D5: the app is the only real
 * store) that the popup shows while the app is not connected.
 */
export class KnownSettings {
  readonly #deps: Pick<HubDeps, 'cache' | 'mediaButtonSetting'>;
  readonly #now: () => number;
  /** Called after every change the popup should see. */
  readonly #changed: () => void;
  #live: ExtensionSettings | null = null;
  #queues: QueueSummary[] = [];
  #cached: CachedSettings | null = null;
  /** The "Show download button on videos" value last published to content scripts. */
  #buttonSetting: boolean | null = null;

  constructor(
    deps: Pick<HubDeps, 'cache' | 'mediaButtonSetting'>,
    now: () => number,
    changed: () => void,
  ) {
    this.#deps = deps;
    this.#now = now;
    this.#changed = changed;
  }

  /** The connected app's settings; null while it is not connected. */
  get live(): ExtensionSettings | null {
    return this.#live;
  }

  /** The cached copy, from this run or an earlier one; null before anything was cached. */
  get cached(): CachedSettings | null {
    return this.#cached;
  }

  /** The app's settings, or the cached copy when the app is not connected; null before either is known. */
  known(): ExtensionSettings | null {
    return this.#live ?? this.#cached?.settings ?? null;
  }

  /** What the popup shows: the live values while connected, otherwise the cached copy. */
  view(connected: boolean): SettingsView {
    const live = connected ? this.#live : null;
    return {
      settings: live ?? this.#cached?.settings ?? null,
      settingsFromCache: live === null && this.#cached !== null,
      cachedAt: this.#cached?.savedAt ?? null,
      queues: live !== null ? this.#queues : (this.#cached?.queues ?? []),
    };
  }

  /** Takes the cache read at start; settings fetched from the app while it was read win over it. */
  loaded(cached: CachedSettings | null): void {
    if (this.#cached === null) this.#cached = cached;
  }

  adopt(settings: ExtensionSettings): void {
    this.#live = settings;
    this.#save();
    this.#publishButtonSetting(settings.show_media_button);
    this.#changed();
  }

  /** The app's answers after connecting; null for an answer of another kind. */
  refreshed(settings: ExtensionSettings | null, queues: QueueSummary[] | null): void {
    if (queues !== null) this.#queues = queues;
    if (settings !== null) this.adopt(settings);
    else this.#changed();
  }

  adoptQueues(queues: QueueSummary[]): void {
    this.#queues = queues;
    this.#save();
    this.#changed();
  }

  /** The connection is gone; when the person closed the app, the cache keeps that for the next wake. */
  disconnected(appClosed: boolean): void {
    this.#live = null;
    if (appClosed) this.#rememberClosed();
  }

  #rememberClosed(): void {
    if (this.#cached === null || this.#cached.appClosed) return;
    this.#cached = { ...this.#cached, appClosed: true };
    this.#deps.cache.save(this.#cached).catch(() => {
      // Not kept: the next wake connects once more to find out.
    });
  }

  #save(): void {
    if (this.#live === null) return;
    const value: CachedSettings = {
      settings: this.#live,
      queues: this.#queues,
      savedAt: this.#now(),
      appClosed: false,
    };
    this.#cached = value;
    this.#deps.cache.save(value).catch(() => {
      // The cache is only for display; a failed write changes nothing else.
    });
  }

  #publishButtonSetting(on: boolean): void {
    if (this.#buttonSetting === on) return;
    this.#buttonSetting = on;
    this.#deps.mediaButtonSetting?.save(on).catch(() => {
      // Not saved: the buttons keep the previous setting until the next change.
    });
  }
}
