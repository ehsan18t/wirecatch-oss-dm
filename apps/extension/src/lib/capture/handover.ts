import type { AddAction, AddedDownload, ExtensionSettings, NewDownload } from '@wirecatch/protocol';
import { isConfirmed } from '../protocol';
import { shouldCapture, type CaptureCandidate, type SkipReason } from './should-capture';

/** The browser download controls the handover needs (`browser.downloads`). */
export interface DownloadControl {
  pause(id: number): Promise<void>;
  resume(id: number): Promise<void>;
  cancel(id: number): Promise<void>;
  /** Removes the entry from the browser's download list (not the file; there is none after cancel). */
  erase(id: number): Promise<void>;
}

export interface HandoverDeps {
  downloads: DownloadControl;
  /** The app's current settings, waiting briefly for the connection; null when the app cannot be reached. */
  liveSettings(): Promise<ExtensionSettings | null>;
  /** Sends `add_download`. Rejects on an app error, a lost connection or a timeout. */
  addDownload(download: NewDownload): Promise<AddedDownload>;
  /** The `Cookie` header for a URL, or null. Never rejects. */
  cookieHeader(url: string, storeId: string | null): Promise<string | null>;
  userAgent: string | null;
}

/** Why the browser kept a download it was about to hand over. */
export type KeptReason =
  /** The browser would not pause it (usually because it already finished). */
  | 'pause_failed'
  /** The app could not be reached. */
  | 'app_unreachable'
  /** The app's current settings say to leave it in the browser. */
  | SkipReason
  /** The app answered with an error, or not in time. */
  | 'app_refused';

export type HandoverResult =
  | {
      kind: 'handed_over';
      action: AddAction;
      /** False if the browser copy could not be cancelled; it then stays paused in the browser list. */
      browserCopyRemoved: boolean;
    }
  | {
      kind: 'kept';
      reason: KeptReason;
      detail: string | null;
      /** False if the browser would not resume it; it then waits, paused, in the browser's download list. */
      resumed: boolean;
    };

export interface HandoverInput {
  downloadId: number;
  candidate: CaptureCandidate;
  /** Firefox container or private cookie store the download belongs to. */
  cookieStoreId: string | null;
}

/**
 * Moves a browser download to the app without ever losing it:
 *
 * 1. Pause the browser download. If that fails, leave it alone.
 * 2. Check the app's live settings again and build the request (URL, cookies, referrer, user agent).
 * 3. Send `add_download`.
 * 4. Only when the app confirms (started, queued or awaiting confirmation): cancel and erase the browser copy.
 * 5. On any error, refusal or timeout: resume the browser download.
 */
export async function handOver(input: HandoverInput, deps: HandoverDeps): Promise<HandoverResult> {
  const { downloadId, candidate } = input;
  try {
    await deps.downloads.pause(downloadId);
  } catch (error) {
    return { kind: 'kept', reason: 'pause_failed', detail: messageOf(error), resumed: true };
  }

  const keep = async (reason: KeptReason, detail: string | null): Promise<HandoverResult> => {
    let resumed = true;
    try {
      await deps.downloads.resume(downloadId);
    } catch {
      resumed = false;
    }
    return { kind: 'kept', reason, detail, resumed };
  };

  let added: AddedDownload;
  try {
    const settings = await deps.liveSettings();
    if (settings === null) return await keep('app_unreachable', null);
    const decision = shouldCapture(candidate, settings);
    if (!decision.capture) return await keep(decision.reason, null);

    const download: NewDownload = {
      url: candidate.url,
      referrer: candidate.referrer,
      page_url: candidate.referrer,
      suggested_name: candidate.fileName,
      mime: candidate.mime,
      size_hint: candidate.size,
      cookies: await deps.cookieHeader(candidate.url, input.cookieStoreId),
      user_agent: deps.userAgent,
      extra_headers: [],
      source: 'browser_download',
      queue: settings.default_queue,
    };
    added = await deps.addDownload(download);
  } catch (error) {
    return keep('app_refused', messageOf(error));
  }

  if (!isConfirmed(added.action)) {
    return keep('app_refused', `Unexpected answer from Wirecatch: ${added.action}`);
  }

  let browserCopyRemoved = true;
  try {
    await deps.downloads.cancel(downloadId);
  } catch {
    browserCopyRemoved = false;
  }
  if (browserCopyRemoved) {
    try {
      await deps.downloads.erase(downloadId);
    } catch {
      // Cancelled but still listed: harmless, the person can clear it from the list.
    }
  }
  return { kind: 'handed_over', action: added.action, browserCopyRemoved };
}

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
