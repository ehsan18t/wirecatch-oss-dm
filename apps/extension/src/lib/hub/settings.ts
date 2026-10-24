import type { BrowserControls, ExtensionSettings, QueueSummary } from '@wirecatch/protocol';
import { fallback } from '../diag/log';
import type { Snapshot } from '../messages';
import type { CachedSettings, HubDeps } from './deps';

/** The part of the popup's snapshot that comes from the settings. */
export type SettingsView = Pick<Snapshot, 'settings' | 'controls' | 'queues'>;

/**
 * The app's settings and queues as the background knows them (D61): live from a connected
 * app, which pushes every change, and the copy kept for the browser session, which judges a
 * download while the app is still connecting. The popup shows only the live values, so it never
 * shows a setting the app may have changed since.
 */
export class KnownSettings {
  readonly #deps: Pick<HubDeps, 'cache' | 'mediaButtonSetting'>;
  /** Called after every change the popup should see. */
  readonly #changed: () => void;
  #live: ExtensionSettings | null = null;
  #controls: BrowserControls | null = null;
  #queues: QueueSummary[] = [];
  #cached: CachedSettings | null = null;
  /** The "Show download button on videos" value last told to the media buttons. */
  #buttonSetting: boolean | null = null;

  constructor(deps: Pick<HubDeps, 'cache' | 'mediaButtonSetting'>, changed: () => void) {
    this.#deps = deps;
    this.#changed = changed;
  }

  /** The connected app's settings; null while it is not connected. */
  get live(): ExtensionSettings | null {
    return this.#live;
  }

  /** The copy kept for this browser session; null before the app was first reached in it. */
  get cached(): CachedSettings | null {
    return this.#cached;
  }

  /** The app's settings, or the session copy when the app is not connected; null before either is known. */
  known(): ExtensionSettings | null {
    return this.#live ?? this.#cached?.settings ?? null;
  }

  /** "Show download button on videos" as far as it is known: on until the app says otherwise, as a fresh install has it. */
  mediaButtonOn(): boolean {
    return this.known()?.show_media_button ?? true;
  }

  /** What the popup shows: the live values while connected, nothing otherwise. */
  view(connected: boolean): SettingsView {
    if (!connected || this.#live === null) return { settings: null, controls: null, queues: [] };
    return { settings: this.#live, controls: this.#controls, queues: this.#queues };
  }

  /** Takes the session copy read at start; settings fetched from the app while it was read win over it. */
  loaded(cached: CachedSettings | null): void {
    if (this.#cached === null) this.#cached = cached;
    // The buttons ask for the setting as they start, so only a change from here on is told.
    this.#buttonSetting ??= this.mediaButtonOn();
  }

  adopt(settings: ExtensionSettings): void {
    this.#live = settings;
    this.#save();
    this.#publishButtonSetting();
    this.#changed();
  }

  adoptControls(controls: BrowserControls): void {
    this.#controls = controls;
    this.#changed();
  }

  /** The app's answers after connecting; null for an answer of another kind, or one the app cannot give. */
  refreshed(
    settings: ExtensionSettings | null,
    controls: BrowserControls | null,
    queues: QueueSummary[] | null,
  ): void {
    if (queues !== null) this.#queues = queues;
    if (controls !== null) this.#controls = controls;
    if (settings === null) {
      this.#changed();
    } else {
      this.adopt(settings);
    }
  }

  adoptQueues(queues: QueueSummary[]): void {
    this.#queues = queues;
    this.#changed();
  }

  /** The connection is gone: what it said stops counting. When the person closed the app, the session copy keeps that for the next wake. */
  disconnected(appClosed: boolean): void {
    this.#live = null;
    this.#controls = null;
    if (appClosed) this.#rememberClosed();
  }

  #rememberClosed(): void {
    if (this.#cached === null || this.#cached.appClosed) return;
    this.#cached = { ...this.#cached, appClosed: true };
    // Not kept, the next wake connects once more to find out.
    this.#deps.cache
      .save(this.#cached)
      .catch(fallback('settings', 'Could not remember that the app is closed', undefined));
  }

  #save(): void {
    if (this.#live === null) return;
    const value: CachedSettings = { settings: this.#live, appClosed: false };
    this.#cached = value;
    // A failed write only means a stopped background asks the app again.
    this.#deps.cache
      .save(value)
      .catch(fallback('settings', 'Could not keep the settings for this session', undefined));
  }

  #publishButtonSetting(): void {
    const on = this.mediaButtonOn();
    if (this.#buttonSetting === on) return;
    this.#buttonSetting = on;
    // Not told, open pages keep the previous setting until the next change or reload.
    this.#deps.mediaButtonSetting
      ?.publish(on)
      .catch(fallback('settings', 'Could not tell the pages about the media button', undefined));
  }
}
