import { describe, expect, it } from 'vitest';
import {
  applyBrowserControlsPatch,
  applyExtensionSettingsPatch,
  normalizeFileType,
  normalizeSite,
} from './extension-settings';
import fixtures from './generated/fixtures.json';
import {
  isBrowserControls,
  isBrowserControlsPatch,
  isExtensionSettings,
  isExtensionSettingsPatch,
} from './generated/validators';

describe('normalizeSite', () => {
  it('keeps every site the way the app keeps it', () => {
    expect(fixtures.normalize_site.length).toBeGreaterThan(0);
    const wrong = fixtures.normalize_site.flatMap(({ typed, site }) => {
      const kept = normalizeSite(typed);
      return kept === site
        ? []
        : [`${JSON.stringify(typed)} kept as ${String(kept)}, not ${String(site)}`];
    });
    expect(wrong).toEqual([]);
  });
});

describe('normalizeFileType', () => {
  it('keeps every file type the way the app keeps it', () => {
    expect(fixtures.normalize_file_type.length).toBeGreaterThan(0);
    for (const { typed, type } of fixtures.normalize_file_type) {
      expect(normalizeFileType(typed), typed).toBe(type);
    }
  });
});

describe('applyBrowserControlsPatch', () => {
  it('applies every patch the way the app applies it', () => {
    expect(fixtures.apply_browser_controls.length).toBeGreaterThan(0);
    for (const { before, patch, after } of fixtures.apply_browser_controls) {
      if (!isBrowserControls(before) || !isBrowserControlsPatch(patch)) {
        throw new Error(
          `A fixture is not controls and a patch: ${JSON.stringify({ before, patch })}`,
        );
      }
      expect(applyBrowserControlsPatch(before, patch), JSON.stringify(patch)).toEqual(after);
    }
  });
});

describe('applyExtensionSettingsPatch', () => {
  it('applies every patch the way the app applies it', () => {
    expect(fixtures.apply_extension_settings.length).toBeGreaterThan(0);
    for (const { before, patch, after } of fixtures.apply_extension_settings) {
      if (!isExtensionSettings(before) || !isExtensionSettingsPatch(patch)) {
        throw new Error(
          `A fixture is not settings and a patch: ${JSON.stringify({ before, patch })}`,
        );
      }
      expect(applyExtensionSettingsPatch(before, patch), JSON.stringify(patch)).toEqual(after);
    }
  });

  it('leaves the settings it was given as they were', () => {
    const [first] = fixtures.apply_extension_settings;
    if (!first || !isExtensionSettings(first.before) || !isExtensionSettingsPatch(first.patch)) {
      throw new Error('The first fixture is not settings and a patch.');
    }
    const before = { ...first.before, ignored_sites: [...first.before.ignored_sites] };
    applyExtensionSettingsPatch(first.before, { ...first.patch, add_ignored_sites: ['new.com'] });
    expect(first.before).toEqual(before);
  });
});
