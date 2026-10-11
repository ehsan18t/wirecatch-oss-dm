import { baseName } from '../file-types';
import type { CaptureCandidate } from './should-capture';

/**
 * The fields this extension reads from a `downloads.DownloadItem`. Chromium and
 * Firefox share most of them; `finalUrl` is Chromium only and `cookieStoreId` is
 * Firefox only.
 */
export interface DownloadItemLike {
  id: number;
  url: string;
  finalUrl?: string;
  referrer?: string;
  /** Chromium: tentative name or absolute path. Firefox: absolute path. */
  filename?: string;
  mime?: string;
  /** Chromium uses 0 for unknown, Firefox uses -1. */
  totalBytes?: number;
  fileSize?: number;
  incognito?: boolean;
  byExtensionId?: string;
  state?: string;
  cookieStoreId?: string;
  /** Chromium: the browser's safety verdict (`safe`, `accepted`, `file`, `url` and so on). */
  danger?: string;
}

function nonEmpty(value: string | undefined): string | null {
  return value === undefined || value === '' ? null : value;
}

function knownSize(item: DownloadItemLike): number | null {
  if (item.totalBytes !== undefined && item.totalBytes > 0) return item.totalBytes;
  if (item.fileSize !== undefined && item.fileSize > 0) return item.fileSize;
  return null;
}

/** Describes a browser download in the browser-neutral shape `shouldCapture` takes. */
export function candidateFromItem(item: DownloadItemLike): CaptureCandidate {
  const finalUrl = nonEmpty(item.finalUrl);
  const url = finalUrl ?? item.url;
  const filename = nonEmpty(item.filename);
  return {
    url,
    originalUrl: url === item.url ? null : item.url,
    referrer: nonEmpty(item.referrer),
    fileName: filename === null ? null : nonEmpty(baseName(filename)),
    mime: nonEmpty(item.mime),
    size: knownSize(item),
    incognito: item.incognito === true,
    startedByExtension: nonEmpty(item.byExtensionId) !== null,
    // Chromium reports `in_progress` while a download runs; Firefox does too.
    inProgress: item.state === undefined || item.state === 'in_progress',
    danger: nonEmpty(item.danger),
  };
}
