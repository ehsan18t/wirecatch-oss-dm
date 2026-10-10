import { describe, expect, it } from 'vitest';
import { defaultSettings } from './backend/mock-data';
import { SECTIONS, SETTINGS, searchSettings } from './settings-schema';

describe('settings schema', () => {
  it('covers every AppSettings and ExtensionSettings field', () => {
    const s = defaultSettings();
    const fields = [
      ...Object.keys(s).filter((k) => k !== 'extension'),
      ...Object.keys(s.extension),
    ];
    const ids = new Set<string>(SETTINGS.map((d) => d.id));
    for (const f of fields) expect(ids, `missing setting for ${f}`).toContain(f);
    expect(ids).toContain('export');
    expect(ids).toContain('import');
  });

  it('is written in plain language with one-line help', () => {
    for (const d of SETTINGS) {
      expect(d.label.length).toBeGreaterThan(3);
      expect(d.help).not.toMatch(/\n/);
      expect(`${d.label} ${d.help}`).not.toMatch(/—/);
      expect(SECTIONS.some((s) => s.id === d.section)).toBe(true);
    }
  });

  it('puts tuning under Advanced', () => {
    const advanced = SETTINGS.filter((d) => d.section === 'advanced').map((d) => d.id);
    expect(advanced).toEqual(['connections_per_download', 'max_concurrent_downloads']);
  });

  it('finds settings by words people use', () => {
    expect(searchSettings('dark').map((d) => d.id)).toEqual(['theme']);
    expect(searchSettings('bandwidth').map((d) => d.id)).toContain('global_speed_limit');
    expect(searchSettings('threads').map((d) => d.id)).toContain('connections_per_download');
    expect(searchSettings('')).toHaveLength(SETTINGS.length);
    expect(searchSettings('zzzz')).toHaveLength(0);
  });
});
