import { describe, expect, it } from 'vitest';
import { defaultSettings } from './backend/mock';
import { SETTINGS } from './settings-schema';
import { searchSettings } from './settings-search';
import { SECTIONS } from './settings-sections';

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
    expect(ids).toContain('queues');
  });

  it('is written in plain language with one-line help', () => {
    for (const d of SETTINGS) {
      expect(d.label.length).toBeGreaterThan(3);
      expect(d.help).not.toMatch(/\n/);
      expect(`${d.label} ${d.help}`).not.toContain(String.fromCharCode(0x2014));
      expect(SECTIONS.some((s) => s.id === d.section)).toBe(true);
    }
  });

  it('puts the limits with the queues, and updates and backup on General (D85)', () => {
    const in_ = (section: string, group: string) =>
      SETTINGS.filter((d) => d.section === section && d.group === group).map((d) => d.id);
    expect(in_('speed', 'Limits')).toEqual([
      'global_speed_limit',
      'max_concurrent_downloads',
      'connections_per_download',
    ]);
    expect(in_('general', 'Updates')).toEqual(['auto_update_addons', 'update_suffix_list']);
    expect(in_('general', 'Backup')).toEqual(['export', 'import']);
    expect(SETTINGS.find((d) => d.section === 'media')?.id).toBe('media_addon');
  });

  it('says what keeping the list of public domains current sends, and where', () => {
    const def = SETTINGS.find((d) => d.id === 'update_suffix_list');
    expect(def?.help).toContain('publicsuffix.org');
    expect(def?.help).toMatch(/IP address/);
    expect(searchSettings('privacy').map((d) => d.id)).toContain('update_suffix_list');
  });

  it('finds settings by words people use', () => {
    expect(searchSettings('dark').map((d) => d.id)).toEqual(['theme']);
    expect(searchSettings('bandwidth').map((d) => d.id)).toContain('global_speed_limit');
    expect(searchSettings('threads').map((d) => d.id)).toContain('connections_per_download');
    expect(searchSettings('')).toHaveLength(SETTINGS.length);
    expect(searchSettings('zzzz')).toHaveLength(0);
  });
});
