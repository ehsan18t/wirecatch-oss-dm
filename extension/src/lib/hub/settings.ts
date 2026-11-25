import type { OptionList, Revision, ShellConfig } from '@wirecatch/protocol';
import { fallback } from '../diag/log';
import type { Snapshot } from '../popup-port';
import type { CachedSettings } from '../cache';
import type { HubDeps } from './deps';

/** The part of the popup's snapshot that comes from the app. */
export type SettingsView = Pick<Snapshot, 'options' | 'privateWindows'>;

/** Whether `next` replaces `held`: one from a new app instance always does, and one from the same instance with a higher counter. */
export function isNewer(next: Revision, held: Revision | null): boolean {
  if (held === null) return true;
  return next.instance !== held.instance || next.counter > held.counter;
}

/**
 * What the app told the background (D280): the values the extension's own checks use, live from
 * a connected app and kept for the browser session, and the popup's options, live only. The popup
 * shows only live values, so it never shows a setting the app may have changed since.
 */
export class KnownSettings {
  readonly #deps: Pick<HubDeps, 'cache' | 'mediaButtonSetting'>;
  /** Called after every change the popup should see. */
  readonly #listeners: (() => void)[] = [];
  readonly #ready: Promise<void>;
  #markReady: () => void = () => undefined;
  #live: ShellConfig | null = null;
  #options: OptionList | null = null;
  #cached: CachedSettings | null = null;
  /** The "Show download button on videos" value last told to the media buttons. */
  #buttonSetting: boolean | null = null;

  constructor(deps: Pick<HubDeps, 'cache' | 'mediaButtonSetting'>) {
    this.#deps = deps;
    this.#ready = new Promise((resolve) => {
      this.#markReady = resolve;
    });
  }

  /** Calls `listener` after every change the popup should see. */
  onChange(listener: () => void): void {
    this.#listeners.push(listener);
  }

  /** Resolves once the session copy was read (`loaded`), so nothing is decided without it. */
  get ready(): Promise<void> {
    return this.#ready;
  }

  /** The connected app's values; null while it is not connected. */
  get live(): ShellConfig | null {
    return this.#live;
  }

  /** The copy kept for this browser session; null before the app was first reached in it. */
  get cached(): CachedSettings | null {
    return this.#cached;
  }

  /** The app's values, or the session copy when the app is not connected; null before either is known. Never a default: an unknown setting is not a setting the person chose. */
  known(): ShellConfig | null {
    return this.#live ?? this.#cached?.settings ?? null;
  }

  /** "Show download button on videos" as far as it is known: on until the app says otherwise, as a fresh install has it. */
  mediaButtonOn(): boolean {
    return this.known()?.show_media_button ?? true;
  }

  /** `mediaButtonOn` for a media button asking as its page loads: once the session copy is in, since the question may be what woke the background, and pages are told only later changes. */
  async mediaButtonOnceLoaded(): Promise<boolean> {
    await this.#ready;
    return this.mediaButtonOn();
  }

  /** "Work in private windows" for what the extension does unasked (watching media, the button, notices): only a known yes counts, and nothing is asked of the app, so a page never starts it (D25.6). */
  async privateWindowsKnownOn(): Promise<boolean> {
    await this.#ready;
    return this.known()?.private_windows === true;
  }

  /** What the popup shows: the live options while connected, nothing otherwise. */
  view(connected: boolean): SettingsView {
    return {
      options: connected ? this.#options : null,
      privateWindows: this.known()?.private_windows === true,
    };
  }

  /** Takes the session copy read at start; values fetched from the app while it was read win over it. */
  loaded(cached: CachedSettings | null): void {
    if (this.#cached === null) this.#cached = cached;
    // The buttons ask for the setting as they start, so only a change from here on is told.
    this.#buttonSetting ??= this.mediaButtonOn();
    this.#markReady();
  }

  /** Takes values from the app, unless they are older than the ones held. */
  adopt(config: ShellConfig): void {
    if (!isNewer(config.revision, this.#live?.revision ?? null)) return;
    this.#live = config;
    this.#save();
    this.#publishButtonSetting();
    this.#changed();
  }

  /** Takes the popup's options, unless they are older than the ones held. */
  adoptOptions(options: OptionList): void {
    if (!isNewer(options.revision, this.#options?.revision ?? null)) return;
    this.#options = options;
    this.#changed();
  }

  /** The connection is gone: what it said stops counting, and the next one starts afresh. When the person closed the app, the session copy keeps that for the next wake. */
  disconnected(appClosed: boolean): void {
    this.#live = null;
    this.#options = null;
    if (appClosed) this.#rememberClosed();
    this.#changed();
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

  #changed(): void {
    for (const listener of this.#listeners) listener();
  }
}
