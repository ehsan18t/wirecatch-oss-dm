import { describe, expect, it } from 'vitest';
import { normalizeFileType, normalizeSite } from './extension-settings';
import fixtures from './generated/fixtures.json';

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
