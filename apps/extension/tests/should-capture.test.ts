import type { ExtensionSettings } from '@wirecatch/protocol';
import { describe, expect, it } from 'vitest';
import { candidateFromItem } from '../src/lib/capture/candidate';
import { shouldCapture, type CaptureCandidate } from '../src/lib/capture/should-capture';
import { DEFAULT_SETTINGS, downloadItem } from './fakes';

function candidate(overrides: Partial<CaptureCandidate> = {}): CaptureCandidate {
  return {
    url: 'https://cdn.example.net/files/report.pdf',
    originalUrl: null,
    referrer: 'https://example.com/page',
    fileName: 'report.pdf',
    mime: 'application/pdf',
    size: 2_000_000,
    incognito: false,
    startedByExtension: false,
    inProgress: true,
    danger: 'safe',
    ...overrides,
  };
}

function settings(overrides: Partial<ExtensionSettings> = {}): ExtensionSettings {
  return { ...DEFAULT_SETTINGS, ...overrides };
}

describe('shouldCapture', () => {
  it('captures an ordinary web download with default settings', () => {
    expect(shouldCapture(candidate(), settings())).toEqual({ capture: true });
  });

  it('is pure: same input, same answer, inputs untouched', () => {
    const item = candidate();
    const s = settings({ ignored_sites: ['other.org'], capture_extensions: ['pdf'] });
    const before = JSON.stringify([item, s]);
    const first = shouldCapture(item, s);
    const second = shouldCapture(item, s);
    expect(first).toEqual(second);
    expect(JSON.stringify([item, s])).toBe(before);
  });

  describe('capture on/off', () => {
    it('keeps every download in the browser when capture is off', () => {
      expect(shouldCapture(candidate(), settings({ capture_enabled: false }))).toEqual({
        capture: false,
        reason: 'capture_off',
      });
    });
  });

  describe('download state and origin', () => {
    it('leaves finished, failed or cancelled downloads alone', () => {
      expect(shouldCapture(candidate({ inProgress: false }), settings())).toEqual({
        capture: false,
        reason: 'not_in_progress',
      });
    });

    it('leaves downloads started by an extension alone (including its own browser fallback)', () => {
      expect(shouldCapture(candidate({ startedByExtension: true }), settings())).toEqual({
        capture: false,
        reason: 'started_by_extension',
      });
    });

    it('leaves private window downloads in the browser', () => {
      expect(shouldCapture(candidate({ incognito: true }), settings())).toEqual({
        capture: false,
        reason: 'private_window',
      });
    });
  });

  describe('addresses', () => {
    it.each([
      'blob:https://example.com/0b8f1c2e',
      'data:text/plain;base64,aGVsbG8=',
      'BLOB:https://x.org/1',
    ])('never captures %s', (url) => {
      expect(shouldCapture(candidate({ url }), settings())).toEqual({
        capture: false,
        reason: 'blob_or_data_url',
      });
    });

    it.each([
      'chrome://settings/',
      'edge://downloads/',
      'about:blank',
      'chrome-extension://abc/file.zip',
      'moz-extension://abc/file.zip',
      'view-source:https://example.com',
    ])('never captures the browser internal address %s', (url) => {
      expect(shouldCapture(candidate({ url }), settings())).toEqual({
        capture: false,
        reason: 'internal_page',
      });
    });

    it('never captures a download started from a browser internal page', () => {
      expect(shouldCapture(candidate({ referrer: 'chrome://downloads/' }), settings())).toEqual({
        capture: false,
        reason: 'internal_page',
      });
    });

    it.each(['file:///C:/Users/me/file.zip', 'ftp://ftp.example.com/file.zip', 'not a url'])(
      'leaves other addresses (%s) in the browser',
      (url) => {
        expect(shouldCapture(candidate({ url }), settings())).toEqual({
          capture: false,
          reason: 'unsupported_url',
        });
      },
    );

    it('captures plain http as well as https', () => {
      expect(shouldCapture(candidate({ url: 'http://example.com/a.zip' }), settings())).toEqual({
        capture: true,
      });
    });
  });

  describe('ignored sites', () => {
    it('keeps downloads whose address is an ignored site', () => {
      const s = settings({ ignored_sites: ['cdn.example.net'] });
      expect(shouldCapture(candidate(), s)).toEqual({ capture: false, reason: 'ignored_site' });
    });

    it('covers subdomains of an ignored site', () => {
      const s = settings({ ignored_sites: ['example.net'] });
      expect(shouldCapture(candidate({ url: 'https://a.b.example.net/x.zip' }), s)).toEqual({
        capture: false,
        reason: 'ignored_site',
      });
    });

    it('does not cover look-alike hosts', () => {
      const s = settings({ ignored_sites: ['example.net'] });
      expect(shouldCapture(candidate({ url: 'https://badexample.net/x.zip' }), s)).toEqual({
        capture: true,
      });
    });

    it('keeps downloads started on an ignored page even when the file comes from elsewhere', () => {
      const s = settings({ ignored_sites: ['example.com'] });
      expect(shouldCapture(candidate({ referrer: 'https://www.example.com/page' }), s)).toEqual({
        capture: false,
        reason: 'ignored_site',
      });
    });

    it('checks the address before redirects too', () => {
      const s = settings({ ignored_sites: ['short.link'] });
      const item = candidate({ originalUrl: 'https://short.link/abc', referrer: null });
      expect(shouldCapture(item, s)).toEqual({ capture: false, reason: 'ignored_site' });
    });

    it('matches entries written loosely (case, scheme, wildcard, trailing slash)', () => {
      for (const entry of [
        'CDN.Example.NET',
        'https://cdn.example.net/',
        '*.example.net',
        'example.net.',
      ]) {
        expect(
          shouldCapture(candidate({ referrer: null }), settings({ ignored_sites: [entry] })),
        ).toEqual({
          capture: false,
          reason: 'ignored_site',
        });
      }
    });

    it('ignores the port when matching', () => {
      const s = settings({ ignored_sites: ['localhost'] });
      expect(
        shouldCapture(candidate({ url: 'http://localhost:8080/a.zip', referrer: null }), s),
      ).toEqual({
        capture: false,
        reason: 'ignored_site',
      });
    });

    it('captures when no ignored site matches', () => {
      const s = settings({ ignored_sites: ['other.org', ''] });
      expect(shouldCapture(candidate(), s)).toEqual({ capture: true });
    });
  });

  describe('file types', () => {
    it('captures every type when the list is empty', () => {
      expect(shouldCapture(candidate({ fileName: 'anything.weird' }), settings())).toEqual({
        capture: true,
      });
    });

    it('captures a listed type', () => {
      const s = settings({ capture_extensions: ['zip', 'pdf'] });
      expect(shouldCapture(candidate(), s)).toEqual({ capture: true });
    });

    it('keeps a type that is not listed', () => {
      const s = settings({ capture_extensions: ['zip', 'exe'] });
      expect(shouldCapture(candidate(), s)).toEqual({ capture: false, reason: 'file_type' });
    });

    it('matches regardless of case, and accepts entries with a dot', () => {
      const s = settings({ capture_extensions: ['.PDF'] });
      expect(shouldCapture(candidate({ fileName: 'REPORT.Pdf' }), s)).toEqual({ capture: true });
    });

    it('matches multi-part types such as tar.gz', () => {
      const s = settings({ capture_extensions: ['tar.gz'] });
      expect(shouldCapture(candidate({ fileName: 'src.tar.gz' }), s)).toEqual({ capture: true });
      expect(shouldCapture(candidate({ fileName: 'src.gz' }), s)).toEqual({
        capture: false,
        reason: 'file_type',
      });
    });

    it('falls back to the name in the address when the browser has none', () => {
      const s = settings({ capture_extensions: ['pdf'] });
      expect(shouldCapture(candidate({ fileName: null }), s)).toEqual({ capture: true });
    });

    it('keeps a file whose type cannot be known when a list is set', () => {
      const s = settings({ capture_extensions: ['pdf'] });
      const item = candidate({ fileName: null, url: 'https://example.com/' });
      expect(shouldCapture(item, s)).toEqual({ capture: false, reason: 'file_type' });
    });
  });

  describe('minimum size', () => {
    it('keeps a file known to be smaller than the minimum', () => {
      const s = settings({ min_capture_size: 5_000_000 });
      expect(shouldCapture(candidate({ size: 4_999_999 }), s)).toEqual({
        capture: false,
        reason: 'too_small',
      });
    });

    it('captures a file exactly at the minimum', () => {
      const s = settings({ min_capture_size: 5_000_000 });
      expect(shouldCapture(candidate({ size: 5_000_000 }), s)).toEqual({ capture: true });
    });

    it('captures a file whose size is unknown', () => {
      const s = settings({ min_capture_size: 5_000_000 });
      expect(shouldCapture(candidate({ size: null }), s)).toEqual({ capture: true });
    });

    it('captures every size when the minimum is 0', () => {
      expect(shouldCapture(candidate({ size: 1 }), settings())).toEqual({ capture: true });
    });
  });

  describe('browser protection', () => {
    it.each(['safe', 'accepted'])('captures a download the browser calls %s', (danger) => {
      expect(shouldCapture(candidate({ danger }), settings())).toEqual({ capture: true });
    });

    it('captures when the browser gives no verdict (Firefox)', () => {
      expect(shouldCapture(candidate({ danger: null }), settings())).toEqual({ capture: true });
    });

    it.each([
      'file',
      'url',
      'content',
      'uncommon',
      'host',
      'unwanted',
      'asyncScanning',
      'blockedTooLarge',
    ])('never captures a download the browser flags as %s, so its warning applies', (danger) => {
      expect(shouldCapture(candidate({ danger }), settings())).toEqual({
        capture: false,
        reason: 'browser_flagged',
      });
    });

    it.each([
      'setup.exe',
      'Setup.EXE',
      'installer.msi',
      'run.bat',
      'run.cmd',
      'old.com',
      'saver.scr',
      'script.ps1',
      'script.vbs',
      'script.js',
      'script.jse',
      'script.wsf',
      'app.hta',
      'shortcut.lnk',
      'tweak.reg',
      'tool.jar',
      'tool.AppImage',
      'pkg.deb',
      'pkg.rpm',
      'install.sh',
      'install.run',
      'app.apk',
      'app.dmg',
      'app.pkg',
      'archive.zip.exe',
      'evil.exe.',
      'evil.exe. . ',
    ])('keeps the program %s in the browser by default', (fileName) => {
      expect(shouldCapture(candidate({ fileName }), settings())).toEqual({
        capture: false,
        reason: 'program',
      });
    });

    it('keeps a program even when the file type list names it', () => {
      const s = settings({ capture_extensions: ['exe'] });
      expect(shouldCapture(candidate({ fileName: 'setup.exe' }), s)).toEqual({
        capture: false,
        reason: 'program',
      });
    });

    it('spots a program by the name in the address when the browser has none', () => {
      const item = candidate({ fileName: null, url: 'https://example.com/get/setup.exe?v=2' });
      expect(shouldCapture(item, settings())).toEqual({ capture: false, reason: 'program' });
    });

    it('spots a program by its media type when the name says nothing', () => {
      const item = candidate({ fileName: 'download', mime: 'application/x-msdownload' });
      expect(shouldCapture(item, settings())).toEqual({ capture: false, reason: 'program' });
    });

    it.each(['setup.exe.zip', 'notes.txt', 'movie.mp4', 'exe', 'program.exe.pdf', 'disk.iso'])(
      'captures %s, which is not a program',
      (fileName) => {
        expect(shouldCapture(candidate({ fileName, mime: null }), settings())).toEqual({
          capture: true,
        });
      },
    );
  });

  it('checks rules in order: capture off wins over everything else', () => {
    const s = settings({ capture_enabled: false, ignored_sites: ['example.com'] });
    expect(shouldCapture(candidate({ url: 'blob:x', incognito: true }), s)).toEqual({
      capture: false,
      reason: 'capture_off',
    });
  });
});

describe('candidateFromItem', () => {
  it('reads a Chromium download item', () => {
    const c = candidateFromItem(
      downloadItem({
        url: 'https://example.com/get?id=1',
        finalUrl: 'https://cdn.example.com/file.zip',
        filename: 'C:\\Users\\me\\Downloads\\file.zip',
        totalBytes: 0,
        fileSize: -1,
        byExtensionId: '',
      }),
    );
    expect(c).toEqual({
      url: 'https://cdn.example.com/file.zip',
      originalUrl: 'https://example.com/get?id=1',
      referrer: 'https://example.com/downloads',
      fileName: 'file.zip',
      mime: 'application/zip',
      size: null,
      incognito: false,
      startedByExtension: false,
      inProgress: true,
      danger: null,
    });
  });

  it('reads a Firefox download item (no finalUrl, -1 for unknown size, absolute path)', () => {
    const c = candidateFromItem({
      id: 3,
      url: 'https://example.org/a.iso',
      referrer: '',
      filename: '/home/me/Downloads/a.iso',
      mime: '',
      totalBytes: -1,
      fileSize: 1234,
      incognito: true,
      state: 'in_progress',
      cookieStoreId: 'firefox-container-1',
    });
    expect(c).toMatchObject({
      url: 'https://example.org/a.iso',
      originalUrl: null,
      referrer: null,
      fileName: 'a.iso',
      mime: null,
      size: 1234,
      incognito: true,
    });
  });

  it('reads the browser verdict', () => {
    expect(candidateFromItem(downloadItem({ danger: 'url' })).danger).toBe('url');
    expect(candidateFromItem(downloadItem({ danger: 'safe' })).danger).toBe('safe');
  });

  it('marks finished items and extension downloads', () => {
    expect(candidateFromItem(downloadItem({ state: 'complete' })).inProgress).toBe(false);
    expect(candidateFromItem(downloadItem({ byExtensionId: 'abc' })).startedByExtension).toBe(true);
  });
});
