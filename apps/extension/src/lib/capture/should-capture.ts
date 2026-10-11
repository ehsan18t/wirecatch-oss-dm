import type { ExtensionSettings } from '@wirecatch/protocol';
import { fileNameFromUrl, isProgramFile, matchesFileTypes } from '../file-types';
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
  /**
   * The browser's verdict on the file (Chromium's `DownloadItem.danger`): `safe`,
   * `accepted` (the person overrode a warning), or a warning such as `file`, `url`,
   * `content`, `uncommon`, `host` or `unwanted`. Null when the browser does not say.
   */
  danger: string | null;
}

/** Why a download stays in the browser. */
export type SkipReason =
  | 'capture_off'
  | 'not_in_progress'
  | 'started_by_extension'
  | 'private_window'
  | 'browser_flagged'
  | 'blob_or_data_url'
  | 'internal_page'
  | 'unsupported_url'
  | 'ignored_site'
  | 'program'
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

/** Addresses that only exist inside the browser. */
const BROWSER_ONLY_SCHEMES = new Set<string | null>(['blob:', 'data:']);

const WEB_SCHEMES = new Set<string | null>(['http:', 'https:']);

/** Browser verdicts that let a download go ahead; anything else is a warning the browser shows itself. */
const NOT_DANGEROUS = new Set(['safe', 'accepted']);

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
 * 3. The browser flagged it (any `danger` other than `safe` or `accepted`): keep, so the browser's warning applies.
 * 4. `blob:` and `data:` addresses only exist inside the browser: keep.
 * 5. Downloads from the browser's own pages, or with any other non-web address: keep.
 * 6. The download address or the page it came from is an ignored site (or a subdomain of one): keep.
 * 7. Programs, installers and scripts (`isProgramFile`): keep, so the browser's own checks
 *    (Safe Browsing, SmartScreen, download warnings) run on them. This holds even when the
 *    file type list names them.
 * 8. A file type list is set and the file does not match it (or its type is unknown): keep.
 * 9. The size is known and below the minimum: keep. An unknown size is captured.
 */
export function shouldCapture(
  item: CaptureCandidate,
  settings: ExtensionSettings,
): CaptureDecision {
  const rule = KEEP_RULES.find(({ keeps }) => keeps(item, settings));
  return rule === undefined ? { capture: true } : { capture: false, reason: rule.reason };
}

/** One of the rules above: when `keeps` is true, the download stays in the browser for `reason`. */
interface KeepRule {
  reason: SkipReason;
  keeps: (item: CaptureCandidate, settings: ExtensionSettings) => boolean;
}

/** The rules of `shouldCapture`, in order; the first that keeps the download decides. */
const KEEP_RULES: readonly KeepRule[] = [
  { reason: 'capture_off', keeps: (_, settings) => !settings.capture_enabled },
  { reason: 'not_in_progress', keeps: (item) => !item.inProgress },
  { reason: 'started_by_extension', keeps: (item) => item.startedByExtension },
  { reason: 'private_window', keeps: (item) => item.incognito },
  {
    reason: 'browser_flagged',
    keeps: (item) => item.danger !== null && !NOT_DANGEROUS.has(item.danger),
  },
  { reason: 'blob_or_data_url', keeps: (item) => BROWSER_ONLY_SCHEMES.has(schemeOf(item.url)) },
  { reason: 'internal_page', keeps: (item) => isInternal(item.url) || isInternal(item.referrer) },
  { reason: 'unsupported_url', keeps: (item) => !WEB_SCHEMES.has(schemeOf(item.url)) },
  {
    reason: 'ignored_site',
    keeps: (item, settings) => onIgnoredSite(item, settings.ignored_sites),
  },
  { reason: 'program', keeps: (item) => isProgramFile(fileNameOf(item), item.mime) },
  {
    reason: 'file_type',
    keeps: (item, settings) => !matchesFileTypes(fileNameOf(item), settings.capture_extensions),
  },
  {
    reason: 'too_small',
    keeps: (item, settings) => item.size !== null && item.size < settings.min_capture_size,
  },
];

/** True when the download address, the address it started with, or the page it came from is an ignored site. */
function onIgnoredSite(item: CaptureCandidate, sites: readonly string[]): boolean {
  return [item.url, item.originalUrl, item.referrer].some(
    (url) => matchingSite(url, sites) !== null,
  );
}

function fileNameOf(item: CaptureCandidate): string | null {
  return item.fileName ?? fileNameFromUrl(item.url);
}
