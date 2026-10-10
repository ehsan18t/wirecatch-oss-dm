import { describe, expect, it } from 'vitest';
import { detectBrowser } from '../src/lib/browser-info';
import { asCachedSettings, CACHE_KEY, storageCache } from '../src/lib/cache';
import { cookieHeader } from '../src/lib/cookies';
import { baseName, fileNameFromUrl, formatFileTypes, parseFileTypes } from '../src/lib/file-types';
import { asPopupRequest } from '../src/lib/messages';
import { classifyDisconnect, describeProblem, type ProblemKind } from '../src/lib/native/status';
import { applyPatch, isConfirmed, patchOf } from '../src/lib/protocol';
import { originPattern } from '../src/lib/site-access';
import { splitSize, toBytes } from '../src/lib/size';
import { hostMatchesSite, normalizeSite, siteForPage } from '../src/lib/sites';
import { DEFAULT_SETTINGS } from './fakes';

describe('sites', () => {
  it('normalizes what a person types', () => {
    expect(normalizeSite('  Example.COM ')).toBe('example.com');
    expect(normalizeSite('https://www.example.com/path?q=1')).toBe('www.example.com');
    expect(normalizeSite('example.com/path')).toBe('example.com');
    expect(normalizeSite('*.example.com')).toBe('example.com');
    expect(normalizeSite('example.com:8080')).toBe('example.com');
    expect(normalizeSite('')).toBeNull();
    expect(normalizeSite('exa mple.com')).toBeNull();
    expect(normalizeSite('https://')).toBeNull();
  });

  it('gives the site for "Ignore this site" without www', () => {
    expect(siteForPage('https://www.example.com/a')).toBe('example.com');
    expect(siteForPage('http://sub.example.com/')).toBe('sub.example.com');
    expect(siteForPage('chrome://newtab/')).toBeNull();
    expect(siteForPage(null)).toBeNull();
    expect(siteForPage(undefined)).toBeNull();
  });

  it('matches a host and its subdomains only', () => {
    expect(hostMatchesSite('example.com', 'example.com')).toBe(true);
    expect(hostMatchesSite('a.example.com', 'example.com')).toBe(true);
    expect(hostMatchesSite('badexample.com', 'example.com')).toBe(false);
  });
});

describe('file types', () => {
  it('parses a typed list', () => {
    expect(parseFileTypes('.ZIP, exe  mp4,zip;*.iso')).toEqual(['zip', 'exe', 'mp4', 'iso']);
    expect(parseFileTypes('  ')).toEqual([]);
    expect(formatFileTypes(['zip', 'exe'])).toBe('zip, exe');
  });

  it('reads names from addresses and paths', () => {
    expect(fileNameFromUrl('https://x.org/a/b/My%20File.zip?x=1')).toBe('My File.zip');
    expect(fileNameFromUrl('https://x.org/a/%E0%A4%A')).toBe('%E0%A4%A');
    expect(fileNameFromUrl('https://x.org/')).toBeNull();
    expect(fileNameFromUrl('nonsense')).toBeNull();
    expect(baseName('C:\\Users\\me\\a.zip')).toBe('a.zip');
    expect(baseName('/home/me/a.zip')).toBe('a.zip');
  });
});

describe('size', () => {
  it('shows bytes in the largest whole unit', () => {
    expect(splitSize(0)).toEqual({ value: 0, unit: 'MB' });
    expect(splitSize(5 * 1024 ** 2)).toEqual({ value: 5, unit: 'MB' });
    expect(splitSize(2 * 1024 ** 3)).toEqual({ value: 2, unit: 'GB' });
    expect(splitSize(1536)).toEqual({ value: 1.5, unit: 'KB' });
    expect(splitSize(1000)).toEqual({ value: 0.98, unit: 'KB' });
  });

  it('turns typed values into bytes', () => {
    expect(toBytes('5', 'MB')).toBe(5 * 1024 ** 2);
    expect(toBytes('1,5', 'KB')).toBe(1536);
    expect(toBytes('', 'MB')).toBe(0);
    expect(toBytes('-1', 'MB')).toBeNull();
    expect(toBytes('abc', 'MB')).toBeNull();
  });
});

describe('protocol helpers', () => {
  it('builds a patch that changes only the given fields', () => {
    expect(patchOf({ ask_before_download: true })).toEqual({
      capture_enabled: null,
      ask_before_download: true,
      show_media_button: null,
      ignored_sites: null,
      add_ignored_sites: null,
      remove_ignored_sites: null,
      capture_extensions: null,
      min_capture_size: null,
      default_queue: null,
    });
    expect(patchOf({ capture_enabled: false, min_capture_size: 0 }).capture_enabled).toBe(false);
    expect(patchOf({ min_capture_size: 0 }).min_capture_size).toBe(0);
    expect(patchOf({ add_ignored_sites: ['a.com'] }).add_ignored_sites).toEqual(['a.com']);
  });

  it('applies a patch like the app does', () => {
    const next = applyPatch(DEFAULT_SETTINGS, patchOf({ ignored_sites: ['a.com'] }));
    expect(next).toEqual({ ...DEFAULT_SETTINGS, ignored_sites: ['a.com'] });
  });

  it('applies site edits in the app order: whole list, then additions, then removals', () => {
    // Mirrors `site_list_edits_merge_instead_of_overwriting` in crates/protocol.
    const start = { ...DEFAULT_SETTINGS, ignored_sites: ['a.com', 'b.com'] };
    const next = applyPatch(
      start,
      patchOf({ add_ignored_sites: ['c.com', 'a.com'], remove_ignored_sites: ['b.com'] }),
    );
    expect(next.ignored_sites).toEqual(['a.com', 'c.com']);
    const replaced = applyPatch(
      start,
      patchOf({ ignored_sites: ['x.com'], add_ignored_sites: ['y.com'], remove_ignored_sites: ['x.com'] }),
    );
    expect(replaced.ignored_sites).toEqual(['y.com']);
  });

  it('turns an address into the site permission pattern', () => {
    expect(originPattern('https://cdn.example.com:8443/a/b.zip?x=1')).toBe('https://cdn.example.com/*');
    expect(originPattern('http://example.com')).toBe('http://example.com/*');
    expect(originPattern('blob:https://example.com/1')).toBeNull();
    expect(originPattern('nonsense')).toBeNull();
  });

  it('treats only known actions as a confirmed handover', () => {
    expect(isConfirmed('started')).toBe(true);
    expect(isConfirmed('queued')).toBe(true);
    expect(isConfirmed('awaiting_confirmation')).toBe(true);
    expect(isConfirmed('something_new' as 'started')).toBe(false);
  });
});

describe('connection problems', () => {
  it('recognizes a missing native host in each browser', () => {
    expect(classifyDisconnect('Specified native messaging host not found.')).toBe('host_missing');
    expect(classifyDisconnect('No such native application app.wirecatch.bridge')).toBe('host_missing');
    expect(classifyDisconnect('Access to the specified native messaging host is forbidden.')).toBe(
      'host_missing',
    );
    expect(classifyDisconnect('Native host has exited.')).toBe('disconnected');
    expect(classifyDisconnect(null)).toBe('disconnected');
  });

  it('explains every problem in plain language without em dashes', () => {
    const kinds: ProblemKind[] = ['host_missing', 'app_unavailable', 'version_mismatch', 'disconnected'];
    for (const kind of kinds) {
      const text = describeProblem(kind);
      expect(text.title.length).toBeGreaterThan(10);
      expect(text.fix.length).toBeGreaterThan(10);
      expect(`${text.title}${text.fix}`).not.toContain('\u2014');
    }
  });
});

describe('browser detection', () => {
  it('uses Firefox browser info when present', () => {
    expect(detectBrowser({ firefox: { name: 'Firefox', version: '141.0' } })).toEqual({
      name: 'firefox',
      version: '141.0',
    });
  });

  it('reads Chromium brands', () => {
    const brands = [
      { brand: 'Not)A;Brand', version: '99' },
      { brand: 'Microsoft Edge', version: '140' },
      { brand: 'Chromium', version: '140' },
    ];
    expect(detectBrowser({ brands })).toEqual({ name: 'edge', version: '140' });
    expect(detectBrowser({ brands: [{ brand: 'Google Chrome', version: '141' }] })).toEqual({
      name: 'chrome',
      version: '141',
    });
    expect(detectBrowser({ brands: [{ brand: 'Chromium', version: '139' }] })).toEqual({
      name: 'chromium',
      version: '139',
    });
  });

  it('spots Brave, which hides from the brand list', () => {
    const brands = [{ brand: 'Chromium', version: '140' }];
    expect(detectBrowser({ brands, isBrave: true })).toEqual({ name: 'brave', version: '140' });
  });

  it('falls back to the user agent', () => {
    const ua = 'Mozilla/5.0 (Windows NT 10.0) AppleWebKit/537.36 Chrome/140.0.0.0 Safari/537.36 OPR/124.0';
    expect(detectBrowser({ userAgent: ua })).toEqual({ name: 'opera', version: '124.0' });
    expect(detectBrowser({ userAgent: 'Something/1.0' })).toEqual({ name: 'unknown', version: null });
  });
});

describe('cookies', () => {
  it('builds a Cookie header value', () => {
    expect(
      cookieHeader([
        { name: 'session', value: 'abc' },
        { name: 'theme', value: 'dark' },
      ]),
    ).toBe('session=abc; theme=dark');
    expect(cookieHeader([{ name: '', value: 'bare' }])).toBe('bare');
    expect(cookieHeader([])).toBeNull();
  });
});

describe('settings cache', () => {
  it('reads back what it saved and rejects anything else', async () => {
    const store: Record<string, unknown> = {};
    const cache = storageCache({
      get: (key) => Promise.resolve({ [key]: store[key] }),
      set: (items) => {
        Object.assign(store, items);
        return Promise.resolve();
      },
    });
    expect(await cache.load()).toBeNull();
    const value = {
      settings: DEFAULT_SETTINGS,
      queues: [{ id: 1, name: 'Main', is_default: true }],
      savedAt: 5,
    };
    await cache.save(value);
    expect(store[CACHE_KEY]).toEqual(value);
    expect(await cache.load()).toEqual(value);
    expect(asCachedSettings({ settings: { capture_enabled: 'yes' }, savedAt: 1 })).toBeNull();
    expect(asCachedSettings('nope')).toBeNull();
    expect(asCachedSettings({ settings: DEFAULT_SETTINGS, savedAt: 1, queues: [{ id: 'x' }] })).toEqual({
      settings: DEFAULT_SETTINGS,
      queues: [],
      savedAt: 1,
    });
  });
});

describe('popup messages', () => {
  it('accepts only well-formed requests', () => {
    expect(asPopupRequest({ id: 1, command: { type: 'pause_all' } })).not.toBeNull();
    expect(
      asPopupRequest({ id: 1, command: { type: 'update_settings', patch: patchOf({}) } }),
    ).not.toBeNull();
    expect(asPopupRequest({ id: 1, command: { type: 'update_settings' } })).toBeNull();
    expect(asPopupRequest({ id: '1', command: { type: 'pause_all' } })).toBeNull();
    expect(asPopupRequest({ id: 1, command: { type: 'delete_everything' } })).toBeNull();
    expect(asPopupRequest(null)).toBeNull();
  });
});
