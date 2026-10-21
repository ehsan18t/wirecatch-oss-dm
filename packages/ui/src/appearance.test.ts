import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  ACCENTS,
  applyAppearance,
  DEFAULT_ACCENT,
  isAccent,
  isScheme,
  SYSTEM_ACCENT_PROPERTY,
} from './appearance';

describe('appearance', () => {
  const html = document.documentElement;

  afterEach(() => {
    for (const name of ['data-scheme', 'data-accent', 'data-appearance-changing']) {
      html.removeAttribute(name);
    }
    html.style.removeProperty(SYSTEM_ACCENT_PROPERTY);
  });

  it('defaults to cyan and knows its names', () => {
    expect(DEFAULT_ACCENT).toBe('cyan');
    expect(ACCENTS[0]).toBe('cyan');
    expect(ACCENTS).toContain('system');
    expect(isAccent('lime')).toBe(false);
    expect(isAccent('teal')).toBe(true);
    expect(isAccent('amber')).toBe(true);
    expect(isScheme('dark')).toBe(true);
    expect(isScheme('system')).toBe(false);
  });

  it('writes the contract onto the element, with no crossfade on the first paint', () => {
    applyAppearance(html, { scheme: 'dark', accent: 'system', systemAccent: '#0078d4' });
    expect(html.dataset.scheme).toBe('dark');
    expect(html.dataset.accent).toBe('system');
    expect(html.style.getPropertyValue(SYSTEM_ACCENT_PROPERTY)).toBe('#0078d4');
    expect(html.hasAttribute('data-appearance-changing')).toBe(false);
  });

  it('crossfades a real change for one beat, and not a repeat of the same look', () => {
    vi.useFakeTimers();
    applyAppearance(html, { scheme: 'dark', accent: 'green' });
    applyAppearance(html, { scheme: 'dark', accent: 'green' });
    expect(html.hasAttribute('data-appearance-changing')).toBe(false);
    applyAppearance(html, { scheme: 'light', accent: 'green' });
    expect(html.hasAttribute('data-appearance-changing')).toBe(true);
    vi.runAllTimers();
    expect(html.hasAttribute('data-appearance-changing')).toBe(false);
    vi.useRealTimers();
  });

  it('leaves out a system accent that is not a colour, so the accent falls back to cyan', () => {
    applyAppearance(html, { scheme: 'dark', accent: 'system', systemAccent: 'not-a-colour' });
    expect(html.dataset.accent).toBe('system');
    expect(html.style.getPropertyValue(SYSTEM_ACCENT_PROPERTY)).toBe('');
  });

  it('removes what no longer applies', () => {
    applyAppearance(html, { scheme: 'dark', accent: 'system', systemAccent: 'red' });
    applyAppearance(html, { scheme: null, accent: 'green', systemAccent: 'red' });
    expect(html.hasAttribute('data-scheme')).toBe(false);
    expect(html.dataset.accent).toBe('green');
    expect(html.style.getPropertyValue(SYSTEM_ACCENT_PROPERTY)).toBe('');
  });
});
