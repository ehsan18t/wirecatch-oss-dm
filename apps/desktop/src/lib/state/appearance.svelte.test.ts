// A window's theme and accent on its page (D36): resolved from the settings, the system's light or
// dark and the system's accent colour, kept from the shell's first paint until the settings
// load, and following every change live, in every window, against the mock backend.

import type { SystemAppearance } from '@wirecatch/protocol';
import { flushSync } from 'svelte';
import { describe, expect, it, vi } from 'vitest';
import { appState, mockBackend } from '$lib/testing/setup';
import { SAMPLE_SYSTEM_ACCENT } from '$lib/backend/mock';
import {
  Appearance,
  followLook,
  LOOK_KEY,
  resolveScheme,
  shownAccent,
  SYSTEM_ACCENT_PROPERTY,
  type AppearanceSource,
  type Look,
} from './appearance.svelte';

/** A system light-or-dark preference a test can switch. */
function systemPreference(dark: boolean) {
  const listeners = new Set<(e: MediaQueryListEvent) => void>();
  const query = {
    matches: dark,
    addEventListener: (_: string, listener: (e: MediaQueryListEvent) => void) =>
      listeners.add(listener),
    removeEventListener: (_: string, listener: (e: MediaQueryListEvent) => void) =>
      listeners.delete(listener),
  } as unknown as MediaQueryList;
  const turn = (next: boolean) => {
    for (const listener of listeners) listener({ matches: next } as MediaQueryListEvent);
  };
  return { query, turn, listeners };
}

/** A system that reports `accent` and can change it. */
function systemAccent(accent: string | null) {
  let listener: ((a: SystemAppearance) => void) | null = null;
  const source: AppearanceSource = {
    systemAppearance: () => Promise.resolve({ accent }),
    onSystemAppearance: (l) => {
      listener = l;
      return Promise.resolve(() => (listener = null));
    },
  };
  return {
    source,
    change: (next: string | null) => listener?.({ accent: next }),
    listening: () => listener !== null,
  };
}

function page() {
  return document.createElement('html');
}

describe('the look of a window', () => {
  it('resolves the system theme with the system preference', () => {
    expect(resolveScheme('system', true)).toBe('dark');
    expect(resolveScheme('system', false)).toBe('light');
    expect(resolveScheme('light', true)).toBe('light');
    expect(resolveScheme('dark', false)).toBe('dark');
  });

  it('shows cyan for the system accent where the system has none', () => {
    expect(shownAccent('system', null)).toBe('cyan');
    expect(shownAccent('system', '#0078d4')).toBe('system');
    expect(shownAccent('purple', null)).toBe('purple');
  });

  it('puts the scheme, accent and system colour on the root element', () => {
    const root = page();
    root.style.setProperty(SYSTEM_ACCENT_PROPERTY, '#0078d4');
    const look = new Appearance(root, systemPreference(true).query);
    look.apply({ theme: 'system', accent: 'system' });
    expect(root.dataset.scheme).toBe('dark');
    expect(root.dataset.accent).toBe('system');
    expect(root.style.getPropertyValue(SYSTEM_ACCENT_PROPERTY)).toBe('#0078d4');
    look.apply({ theme: 'light', accent: 'orange' });
    expect(root.dataset.scheme).toBe('light');
    expect(root.dataset.accent).toBe('orange');
  });

  it('crossfades a change of look, but not the settings confirming the first paint (D46)', () => {
    const root = page();
    Object.assign(root.dataset, { scheme: 'dark', accent: 'green' });
    const look = new Appearance(root, systemPreference(false).query);
    look.apply({ theme: 'dark', accent: 'green' });
    expect(root.hasAttribute('data-appearance-changing')).toBe(false);
    look.apply({ theme: 'light', accent: 'green' });
    expect(root.hasAttribute('data-appearance-changing')).toBe(true);
  });

  it('keeps what the shell set before the first paint until the settings load', () => {
    const root = page();
    Object.assign(root.dataset, { scheme: 'dark', accent: 'green' });
    const look = new Appearance(root, systemPreference(false).query);
    look.apply(undefined);
    expect(Object.fromEntries(Object.entries(root.dataset))).toEqual({
      scheme: 'dark',
      accent: 'green',
    });
  });

  it('follows the system turning dark and its accent colour changing, until stopped', async () => {
    const root = page();
    const preference = systemPreference(false);
    const accent = systemAccent('#0078d4');
    const look = new Appearance(root, preference.query);
    let settings = $state<Look>({ theme: 'system', accent: 'system' });
    const cleanup = $effect.root(() => followLook(look, () => settings));
    const stop = await look.connect(accent.source);
    flushSync();
    expect(root.dataset.scheme).toBe('light');
    expect(root.style.getPropertyValue(SYSTEM_ACCENT_PROPERTY)).toBe('#0078d4');

    preference.turn(true);
    accent.change('#e81123');
    flushSync();
    expect(root.dataset.scheme).toBe('dark');
    expect(root.style.getPropertyValue(SYSTEM_ACCENT_PROPERTY)).toBe('#e81123');

    // A system with no accent colour any more shows cyan.
    accent.change(null);
    flushSync();
    expect(root.dataset.accent).toBe('cyan');
    expect(root.style.getPropertyValue(SYSTEM_ACCENT_PROPERTY)).toBe('');

    settings = { theme: 'light', accent: 'pink' };
    flushSync();
    expect([root.dataset.scheme, root.dataset.accent]).toEqual(['light', 'pink']);

    stop();
    expect(preference.listeners.size).toBe(0);
    expect(accent.listening()).toBe(false);
    cleanup();
  });

  it('remembers the look it shows for the next first paint', () => {
    localStorage.clear();
    const look = new Appearance(page(), systemPreference(false).query, localStorage);
    look.apply({ theme: 'dark', accent: 'amber' });
    expect(JSON.parse(localStorage.getItem(LOOK_KEY) ?? 'null')).toEqual({
      theme: 'dark',
      accent: 'amber',
    });
    localStorage.clear();
  });

  it('keeps the first paint colour when the backend cannot say', async () => {
    const root = page();
    root.style.setProperty(SYSTEM_ACCENT_PROPERTY, '#0078d4');
    const look = new Appearance(root, systemPreference(false).query);
    await look.connect({
      systemAppearance: () => Promise.reject(new Error('gone')),
      onSystemAppearance: () => Promise.resolve(() => undefined),
    });
    expect(look.systemAccent).toBe('#0078d4');
  });
});

describe('the main window', () => {
  it('shows the theme and accent changed in any window, and the sample system accent', async () => {
    const backend = mockBackend({ empty: true });
    const app = appState(backend);
    const cleanup = $effect.root(() => followLook(app.appearance, () => app.settings));
    await app.init();
    await vi.waitFor(() => expect(app.appearance.systemAccent).toBe(SAMPLE_SYSTEM_ACCENT));
    const root = document.documentElement;

    // Another window (a download window, say) changes the settings: every window gets them.
    await backend.changeSettings({ theme: 'dark', accent: 'system' });
    flushSync();
    expect(root.dataset.scheme).toBe('dark');
    expect(root.dataset.accent).toBe('system');
    expect(root.style.getPropertyValue(SYSTEM_ACCENT_PROPERTY)).toBe(SAMPLE_SYSTEM_ACCENT);

    await backend.changeSettings({ theme: 'light', accent: 'teal' });
    flushSync();
    expect([root.dataset.scheme, root.dataset.accent]).toEqual(['light', 'teal']);
    cleanup();
    app.dispose();
  });
});
