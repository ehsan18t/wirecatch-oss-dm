import type { ExtensionSettings } from '@wirecatch/protocol';
import { fileNameFromUrl, matchesFileTypes } from '../file-types';
import { matchingSite } from '../sites';

/**
 * A browser download described in browser-neutral terms. Built from a Chromium or
 * Firefox `DownloadItem` by `candidateFromItem`.
 */
export interface CaptureCandidate {
  /** Address the file is served from (after redirects when the browser knows it). */
  url: string;
  /** Address the download started with, when it differs from `url`. */
  originalUrl: string | null;
  /** The page that started the download. */
  referrer: string | null;
  /** File name the browser picked, without any folder. */
  fileName: string | null;
  mime: string | null;
  /** Size in bytes when the server announced it, otherwise null. */
  size: number | null;
  /** Started in a private or incognito window. */
  incognito: boolean;
  /** Started by an extension (including this one) rather than by a web page. */
  startedByExtension: boolean;
  /** Still downloading; finished, failed or cancelled downloads are left alone. */
  inProgress: boolean;
}

/** Why a download stays in the browser. */
export type SkipReason =
  | 'capture_off'
  | 'not_in_progress'
  | 'started_by_extension'
  | 'private_window'
  | 'blob_or_data_url'
  | 'internal_page'
  | 'unsupported_url'
  | 'ignored_site'
  | 'file_type'
  | 'too_small';

export type CaptureDecision = { capture: true } | { capture: false; reason: SkipReason };

/** Schemes of the browser's own pages and of extension pages. */
const INTERNAL_SCHEMES = new Set([
  'about:',
  'chrome:',
  'chrome-extension:',
  'chrome-search:',
  'chrome-untrusted:',
  'devtools:',
  'edge:',
  'brave:',
  'opera:',
  'vivaldi:',
  'moz-extension:',
  'resource:',
  'view-source:',
]);

function schemeOf(url: string): string | null {
  const match = /^([a-z][a-z0-9+.-]*:)/i.exec(url.trim());
  return match?.[1]?.toLowerCase() ?? null;
}

function isInternal(url: string | null): boolean {
  if (url === null || url === '') return false;
  const scheme = schemeOf(url);
  return scheme !== null && INTERNAL_SCHEMES.has(scheme);
}

/**
 * Decides whether Wirecatch should take over a browser download. Pure: the same
 * input always gives the same answer and nothing outside is read or changed.
 *
 * Rules, in order:
 * 1. Capture switched off: keep.
 * 2. Finished, failed or cancelled already, started by an extension, or in a private window: keep.
 * 3. `blob:` and `data:` addresses only exist inside the browser: keep.
 * 4. Downloads from the browser's own pages, or with any other non-web address: keep.
 * 5. The download address or the page it came from is an ignored site (or a subdomain of one): keep.
 * 6. A file type list is set and the file does not match it (or its type is unknown): keep.
 * 7. The size is known and below the minimum: keep. An unknown size is captured.
 */
export function shouldCapture(item: CaptureCandidate, settings: ExtensionSettings): CaptureDecision {
  if (!settings.capture_enabled) return skip('capture_off');
  if (!item.inProgress) return skip('not_in_progress');
  if (item.startedByExtension) return skip('started_by_extension');
  if (item.incognito) return skip('private_window');

  const scheme = schemeOf(item.url);
  if (scheme === 'blob:' || scheme === 'data:') return skip('blob_or_data_url');
  if (isInternal(item.url) || isInternal(item.referrer)) return skip('internal_page');
  if (scheme !== 'http:' && scheme !== 'https:') return skip('unsupported_url');

  const sites = settings.ignored_sites;
  if (
    sites.length > 0 &&
    (matchingSite(item.url, sites) !== null ||
      matchingSite(item.originalUrl, sites) !== null ||
      matchingSite(item.referrer, sites) !== null)
  ) {
    return skip('ignored_site');
  }

  const fileName = item.fileName ?? fileNameFromUrl(item.url);
  if (!matchesFileTypes(fileName, settings.capture_extensions)) return skip('file_type');

  if (item.size !== null && item.size < settings.min_capture_size) return skip('too_small');

  return { capture: true };
}

function skip(reason: SkipReason): CaptureDecision {
  return { capture: false, reason };
}
