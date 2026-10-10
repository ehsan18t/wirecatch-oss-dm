import type {
  AddAction,
  AddedDownload,
  ExtensionSettings,
  NewDownload,
  WithdrawOutcome,
} from '@wirecatch/protocol';
import { RequestError } from '../native/client';
import { isConfirmed } from '../protocol';
import { shouldCapture, type CaptureCandidate, type SkipReason } from './should-capture';

/** What the browser reports about one download (`downloads.search({ id })`). */
export interface DownloadState {
  state: string;
  paused: boolean;
}

/** The browser download controls the handover needs (`browser.downloads`). */
export interface DownloadControl {
  pause(id: number): Promise<void>;
  resume(id: number): Promise<void>;
  cancel(id: number): Promise<void>;
  /** Removes the entry from the browser's download list (not the file; there is none after cancel). */
  erase(id: number): Promise<void>;
  /** The download's current state, or null when the browser no longer lists it. */
  state(id: number): Promise<DownloadState | null>;
}

/** Settings to judge a download by, and whether they came from the app just now or from the display cache. */
export interface JudgingSettings {
  settings: ExtensionSettings;
  fromApp: boolean;
}

/** Sends `add_download` and, when the answer is lost, `withdraw_download`. */
export interface SubmitDeps {
  /** Rejects with a `RequestError` on an app error, a lost connection or a timeout. */
  addDownload(download: NewDownload): Promise<AddedDownload>;
  /** Asks the app to forget a handover (short time limit). Rejects if that fails or times out. */
  withdraw(handoverId: string): Promise<WithdrawOutcome>;
}

export type SubmitResult =
  | { taken: true; action: AddAction; via: 'answer' | 'withdraw' }
  | {
      taken: false;
      reason: 'refused' | 'no_answer';
      detail: string | null;
      /** The app may still have it (the withdraw failed), so it may end up in both places. */
      uncertain: boolean;
    };

/**
 * Offers a download to the app. When no answer comes back (timeout or lost
 * connection), asks the app to withdraw it, so the result is known either way:
 * `withdrawn` or `not_found` means the app does not have it; `already_started`
 * means it does. If even the withdraw fails, the answer is "not taken, uncertain":
 * the caller keeps its copy, because two copies beat none.
 */
export async function submit(download: NewDownload, deps: SubmitDeps): Promise<SubmitResult> {
  let added: AddedDownload;
  try {
    added = await deps.addDownload(download);
  } catch (error) {
    const lost = error instanceof RequestError && (error.kind === 'timeout' || error.kind === 'disconnected');
    if (!lost) return { taken: false, reason: 'refused', detail: messageOf(error), uncertain: false };
    let outcome: WithdrawOutcome;
    try {
      outcome = await deps.withdraw(download.handover_id);
    } catch (withdrawError) {
      return { taken: false, reason: 'no_answer', detail: messageOf(withdrawError), uncertain: true };
    }
    if (outcome === 'already_started') return { taken: true, action: 'started', via: 'withdraw' };
    return { taken: false, reason: 'no_answer', detail: messageOf(error), uncertain: false };
  }
  if (!isConfirmed(added.action)) {
    return {
      taken: false,
      reason: 'refused',
      detail: `Unexpected answer from Wirecatch: ${added.action}`,
      uncertain: false,
    };
  }
  return { taken: true, action: added.action, via: 'answer' };
}

export interface HandoverDeps extends SubmitDeps {
  downloads: DownloadControl;
  /**
   * The settings to judge by after pausing: the app's live ones (waiting briefly for
   * the connection), or the cached ones when the app cannot be reached; null if neither.
   */
  liveSettings(): Promise<JudgingSettings | null>;
  /** The `Cookie` header for a URL, or null. Never rejects. */
  cookieHeader(url: string, storeId: string | null): Promise<string | null>;
  userAgent: string | null;
  newHandoverId(): string;
}

/** Why the browser kept a download it was about to hand over. */
export type KeptReason =
  /** The browser would not pause it (usually because it already finished). */
  | 'pause_failed'
  /** The app could not be reached. */
  | 'app_unreachable'
  /** The settings say to leave it in the browser. */
  | SkipReason
  /** The app answered with an error or an answer this version does not know. */
  | 'app_refused'
  /** No answer came back, and the app then withdrew it (or could not be asked). */
  | 'no_answer';

/** What happened to the browser's copy of a kept download. */
export type BrowserCopy =
  /** Never paused. */
  | 'untouched'
  | 'resumed'
  /** Still paused in the browser's list: the person has to resume it. */
  | 'stuck_paused'
  /** No longer running in the browser (the person cancelled it, or it finished). */
  | 'gone';

export type HandoverResult =
  | {
      kind: 'handed_over';
      handoverId: string;
      action: AddAction;
      /** False if the browser copy could not be cancelled; it then stays paused in the browser list. */
      browserCopyRemoved: boolean;
    }
  | {
      kind: 'kept';
      handoverId: string | null;
      reason: KeptReason;
      detail: string | null;
      browserCopy: BrowserCopy;
      /** The app may have taken it after all (see `SubmitResult.uncertain`). */
      uncertain: boolean;
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
 * 2. Check the settings again (the app's live ones when it can be reached) and build the request.
 * 3. Send `add_download` with a fresh handover id; if no answer comes back, withdraw it (see `submit`).
 * 4. Only when the app has it (started, queued or awaiting confirmation): cancel and erase the browser copy.
 *    `queued` and `awaiting_confirmation` mean the app has not tried the link yet; see `isConfirmed`.
 * 5. Otherwise resume the browser download.
 */
export async function handOver(input: HandoverInput, deps: HandoverDeps): Promise<HandoverResult> {
  const { downloadId, candidate } = input;
  try {
    await deps.downloads.pause(downloadId);
  } catch (error) {
    return {
      kind: 'kept',
      handoverId: null,
      reason: 'pause_failed',
      detail: messageOf(error),
      browserCopy: 'untouched',
      uncertain: false,
    };
  }

  const keep = async (
    handoverId: string | null,
    reason: KeptReason,
    detail: string | null,
    uncertain = false,
  ): Promise<HandoverResult> => ({
    kind: 'kept',
    handoverId,
    reason,
    detail,
    browserCopy: await resumeBrowserCopy(deps.downloads, downloadId),
    uncertain,
  });

  const judging = await deps.liveSettings();
  if (judging === null) return keep(null, 'app_unreachable', null);
  const decision = shouldCapture(candidate, judging.settings);
  if (!decision.capture) return keep(null, decision.reason, null);
  if (!judging.fromApp) return keep(null, 'app_unreachable', null);

  const handoverId = deps.newHandoverId();
  const download: NewDownload = {
    handover_id: handoverId,
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
    queue: judging.settings.default_queue,
  };
  const result = await submit(download, deps);
  if (!result.taken) {
    return keep(
      handoverId,
      result.reason === 'refused' ? 'app_refused' : 'no_answer',
      result.detail,
      result.uncertain,
    );
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
  return { kind: 'handed_over', handoverId, action: result.action, browserCopyRemoved };
}

/** Resumes a paused browser download; if that fails, finds out whether it is still waiting or already gone. */
export async function resumeBrowserCopy(downloads: DownloadControl, id: number): Promise<BrowserCopy> {
  try {
    await downloads.resume(id);
    return 'resumed';
  } catch {
    const state = await downloads.state(id).catch(() => null);
    return state !== null && state.state === 'in_progress' ? 'stuck_paused' : 'gone';
  }
}

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
