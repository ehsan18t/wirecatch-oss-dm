import { NATIVE_HOST_NAME } from '@wirecatch/product';
import { isOutcome, queueId, type ErrorCode } from '@wirecatch/protocol';
import { describe, expect, it, vi } from 'vitest';
import { fakeBrowser } from 'wxt/testing/fake-browser';
import { detectBrowser } from '../src/lib/browser-info';
import {
  cookieStoreOf,
  inFlightStore,
  noticeStore,
  toolbarBadge,
} from '../src/lib/browser/adapters';
import { asCachedSettings, CACHE_KEY, storageCache } from '../src/lib/cache';
import { cookieHeader } from '../src/lib/cookies';
import { baseName, fileNameFromUrl, formatFileTypes, parseFileTypes } from '../src/lib/file-types';
import popupCss from '../src/entrypoints/popup/style.css?raw';
import { asPopupRequest, isNotice, type Notice } from '../src/lib/messages';
import {
  classifyDisconnect,
  describeProblem,
  disconnectMessage,
  type ProblemKind,
} from '../src/lib/native/status';
import { applyPatch, patchOf } from '../src/lib/protocol';
import { BADGE_TOKEN, cssColor } from '../src/lib/theme';
import { originPattern } from '../src/lib/site-access';
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

describe('protocol helpers', () => {
  it('reads every error code the app can answer with', () => {
    const codes: Record<ErrorCode, true> = {
      unsupported_version: true,
      invalid_request: true,
      invalid_url: true,
      not_found: true,
      app_unavailable: true,
      app_not_running: true,
      internal: true,
      not_started: true,
      addon_required: true,
      limit_reached: true,
      wrong_state: true,
      update_check_failed: true,
      unsupported_call: true,
    };
    for (const code of Object.keys(codes)) {
      expect(isOutcome({ status: 'err', value: { code, message: 'No.' } }), code).toBe(true);
    }
  });

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
      patchOf({
        ignored_sites: ['x.com'],
        add_ignored_sites: ['y.com'],
        remove_ignored_sites: ['x.com'],
      }),
    );
    expect(replaced.ignored_sites).toEqual(['y.com']);
  });

  it('turns an address into the site permission pattern', () => {
    expect(originPattern('https://cdn.example.com:8443/a/b.zip?x=1')).toBe(
      'https://cdn.example.com/*',
    );
    expect(originPattern('http://example.com')).toBe('http://example.com/*');
    expect(originPattern('blob:https://example.com/1')).toBeNull();
    expect(originPattern('nonsense')).toBeNull();
  });
});

describe('connection problems', () => {
  // The exact strings Chromium reports (extensions/browser/api/messaging/native_message_host.cc).
  it.each([
    ['Specified native messaging host not found.', 'host_missing'],
    ['Failed to start native messaging host.', 'host_missing'],
    ['Access to the specified native messaging host is forbidden.', 'host_untrusted'],
    ['Native host has exited.', 'disconnected'],
    ['Error when communicating with the native messaging host.', 'disconnected'],
  ] as const)('sorts the Chromium message "%s" as %s', (message, kind) => {
    expect(classifyDisconnect(message)).toBe(kind);
  });

  // The exact strings Firefox reports (NativeMessaging.sys.mjs and the subprocess module).
  it.each([
    // No manifest, an unreadable one, or one that leaves this add-on out: one generic message.
    [`No such native application ${NATIVE_HOST_NAME}`, 'host_missing'],
    // Registered, but the program it names is gone.
    [
      'File at path "/home/u/.local/share/wirecatch/bin/wirecatch-bridge" does not exist, or is not executable',
      'host_missing',
    ],
    [
      String.raw`File at path "C:\Users\u\AppData\Local\Wirecatch\wirecatch-bridge.exe" does not exist, or is not a normal file`,
      'host_missing',
    ],
    ['Executable not found: wirecatch-bridge', 'host_missing'],
    // What Firefox logs when the id is not allowed, in case a version passes it on.
    [
      `This extension does not have permission to use native manifest /usr/lib/mozilla/native-messaging-hosts/${NATIVE_HOST_NAME}.json`,
      'host_untrusted',
    ],
    [
      `This extension does not have permission to use native application ${NATIVE_HOST_NAME} (or the application is not installed)`,
      'host_untrusted',
    ],
  ] as const)('sorts the Firefox message "%s" as %s', (message, kind) => {
    expect(classifyDisconnect(message)).toBe(kind);
  });

  it('reads the reason where each browser puts it', () => {
    const missing = `No such native application ${NATIVE_HOST_NAME}`;
    // Firefox: port.error is an Error; runtime.lastError is not set for ports.
    expect(disconnectMessage(undefined, { error: new Error(missing) })).toBe(missing);
    // Firefox, host exited: port.error is null.
    expect(disconnectMessage(undefined, { error: null })).toBeNull();
    // Chromium: runtime.lastError while the listener runs; the port has no error field.
    expect(
      disconnectMessage(
        { message: 'Access to the specified native messaging host is forbidden.' },
        {},
      ),
    ).toBe('Access to the specified native messaging host is forbidden.');
    expect(disconnectMessage(null, null)).toBeNull();
    expect(disconnectMessage({ message: '' }, { error: { message: 42 } })).toBeNull();
    // Firefox's host exit reaches the popup as a lost connection, never as "not installed".
    expect(classifyDisconnect(disconnectMessage(undefined, { error: null }))).toBe('disconnected');
  });

  it('recognizes a missing native host in Firefox and a port closed without a reason', () => {
    expect(classifyDisconnect(`No such native application ${NATIVE_HOST_NAME}`)).toBe(
      'host_missing',
    );
    expect(classifyDisconnect(null)).toBe('disconnected');
  });

  it('gives each problem its own fix, and never suggests a reinstall that would not help', () => {
    expect(describeProblem('host_missing')).toEqual({
      title: 'Wirecatch is not installed, or not set up for this browser.',
      fix: 'Install Wirecatch, or open it once so it can connect to this browser.',
    });
    expect(describeProblem('host_untrusted')).toEqual({
      title: 'This copy of the extension is not the one Wirecatch trusts.',
      fix: 'Install the extension from the Wirecatch website or store, or update Wirecatch.',
    });
    expect(describeProblem('disconnected').fix).toContain('reconnects by itself');
    const kinds: ProblemKind[] = [
      'host_missing',
      'host_untrusted',
      'app_unavailable',
      'version_mismatch',
      'disconnected',
    ];
    for (const kind of kinds) {
      expect(`${describeProblem(kind).title} ${describeProblem(kind).fix}`).not.toMatch(
        /reinstall/i,
      );
    }
  });

  it('explains every problem in plain language without em dashes', () => {
    const kinds: ProblemKind[] = [
      'host_missing',
      'host_untrusted',
      'app_unavailable',
      'version_mismatch',
      'disconnected',
    ];
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
    const ua =
      'Mozilla/5.0 (Windows NT 10.0) AppleWebKit/537.36 Chrome/140.0.0.0 Safari/537.36 OPR/124.0';
    expect(detectBrowser({ userAgent: ua })).toEqual({ name: 'opera', version: '124.0' });
    expect(detectBrowser({ userAgent: 'Something/1.0' })).toEqual({
      name: 'unknown',
      version: null,
    });
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
      queues: [{ id: queueId(1), name: 'Main', is_default: true }],
      savedAt: 5,
      appClosed: true,
    };
    await cache.save(value);
    expect(store[CACHE_KEY]).toEqual(value);
    expect(await cache.load()).toEqual(value);
    // Written before the field existed: the app counts as running, so waking connects once to find out.
    const older = { settings: value.settings, queues: value.queues, savedAt: value.savedAt };
    expect(asCachedSettings(older)?.appClosed).toBe(false);
    expect(asCachedSettings({ settings: { capture_enabled: 'yes' }, savedAt: 1 })).toBeNull();
    expect(asCachedSettings('nope')).toBeNull();
    expect(
      asCachedSettings({ settings: DEFAULT_SETTINGS, savedAt: 1, queues: [{ id: 'x' }] }),
    ).toEqual({
      settings: DEFAULT_SETTINGS,
      queues: [],
      savedAt: 1,
      appClosed: false,
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

  it("checks the fields of a settings change and the media commands' tab", () => {
    const update = (patch: unknown): unknown => ({
      id: 1,
      command: { type: 'update_settings', patch },
    });
    expect(asPopupRequest(update({ add_ignored_sites: ['example.com'] }))).not.toBeNull();
    expect(asPopupRequest(update({ capture_enabled: null }))).not.toBeNull();
    expect(asPopupRequest(update({ capture_enabled: 'yes' }))).toBeNull();
    expect(asPopupRequest(update({ add_ignored_sites: [1] }))).toBeNull();
    expect(asPopupRequest({ id: 1, command: { type: 'watch_media', tabId: 3 } })).not.toBeNull();
    expect(asPopupRequest({ id: 1, command: { type: 'watch_media', tabId: -1 } })).toBeNull();
    expect(asPopupRequest({ id: 1, command: { type: 'download_media', tabId: 3 } })).toBeNull();
    expect(
      asPopupRequest({ id: 1, command: { type: 'download_media', tabId: 3, key: 'k' } }),
    ).not.toBeNull();
  });
});

describe('cookieStoreOf', () => {
  it("reads a Firefox tab's cookie store, and null for a Chromium tab or anything else", () => {
    expect(cookieStoreOf({ id: 1, cookieStoreId: 'firefox-container-1' })).toBe(
      'firefox-container-1',
    );
    expect(cookieStoreOf({ id: 1, incognito: false })).toBeNull();
    expect(cookieStoreOf({ cookieStoreId: 7 })).toBeNull();
    expect(cookieStoreOf(undefined)).toBeNull();
  });
});

describe('notices kept in session storage', () => {
  const notice: Notice = {
    id: 'd4',
    downloadId: 4,
    untilResumed: true,

    urgent: true,
    tone: 'error',
    message: 'big.zip is paused in the browser.',
    at: 5,
  };

  it('reads back only a well-formed notice', () => {
    expect(isNotice(notice)).toBe(true);
    expect(isNotice({ ...notice, tone: 'loud' })).toBe(false);
    expect(isNotice({ tone: 'warning', message: 'No time.' })).toBe(false);
    expect(isNotice(null)).toBe(false);
  });

  it('saves the notices, clears them, and drops those it cannot read', async () => {
    fakeBrowser.reset();
    const store = noticeStore();
    if (store === undefined) throw new Error('no session storage');
    expect(await store.load()).toEqual([]);
    await store.save([notice]);
    expect(await store.load()).toEqual([notice]);
    await store.save([]);
    expect(await store.load()).toEqual([]);
    await fakeBrowser.storage.session.set({ 'wirecatch.notices': [{ tone: 'loud' }, notice] });
    expect(await store.load()).toEqual([notice]);
  });
});

describe('toolbarBadge', () => {
  it('shows a message on the badge in the popup warning color', () => {
    fakeBrowser.reset();
    const setColor = vi
      .spyOn(fakeBrowser.action, 'setBadgeBackgroundColor')
      .mockResolvedValue(undefined);
    toolbarBadge().set('!', 'Wirecatch has a message for you.');
    expect(setColor).toHaveBeenCalledWith({ color: cssColor(BADGE_TOKEN, popupCss) });
  });
});

describe('in-flight downloads kept in session storage', () => {
  it('saves each download with its handover id and drops entries it cannot read', async () => {
    fakeBrowser.reset();
    const store = inFlightStore();
    if (store === undefined) throw new Error('no session storage');
    expect(await store.load()).toEqual([]);
    await store.save([{ id: 5, handoverId: 'h1' }]);
    expect(await store.load()).toEqual([{ id: 5, handoverId: 'h1' }]);
    await fakeBrowser.storage.session.set({
      'wirecatch.inFlight': [7, { id: 8 }, { id: 9, handoverId: 'h9' }],
    });
    expect(await store.load()).toEqual([{ id: 9, handoverId: 'h9' }]);
  });
});
