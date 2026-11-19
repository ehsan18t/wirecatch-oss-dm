import type {
  AddAction,
  AddedDownload,
  CheckAnswer,
  CookiePlan,
  CookieRequest,
  DownloadCheck,
  NewDownload,
  ShellConfig,
  WithdrawOutcome,
} from '@wirecatch/protocol';
import type { CookieStore, DownloadCookies } from '../cookies';
import { RequestError, type RequestErrorKind } from '../native/client';
import { fallback, log } from '../diag/log';
import { messageOf } from '../errors';
import { nonEmpty } from '../text';
import { shouldCapture, type CaptureCandidate, type SkipReason } from './should-capture';
import type { DownloadRoute } from './browser-route';

/** What the browser reports about one download (`downloads.search({ id })`). */
export interface DownloadState {
  state: string;
  paused: boolean;
  /** Chromium: the browser's safety verdict now (see `CaptureCandidate.danger`). */
  danger?: string;
  /** Where the browser saves it; empty or missing while the browser has not chosen yet. */
  filename?: string;
}

/** The browser download controls the handover needs (`browser.downloads`). */
export interface DownloadControl {
  pause: (id: number) => Promise<void>;
  resume: (id: number) => Promise<void>;
  cancel: (id: number) => Promise<void>;
  /** Removes the entry from the browser's download list (not the file; there is none after cancel). */
  erase: (id: number) => Promise<void>;
  /** The download's current state, or null when the browser no longer lists it. */
  state: (id: number) => Promise<DownloadState | null>;
}

/** Values to judge a download by, and whether they came from the app just now or from the session copy (D61). */
export interface JudgingSettings {
  settings: ShellConfig;
  fromApp: boolean;
}

/** Sends `add_download` and, when the answer is lost, `withdraw_download`. */
export interface SubmitDeps {
  /** Rejects with a `RequestError` on an app error, a lost connection, a timeout or an answer that cannot be read. */
  addDownload: (download: NewDownload) => Promise<AddedDownload>;
  /** Asks the app to forget a handover (short time limit). Rejects if that fails or times out. */
  withdraw: (handoverId: string) => Promise<WithdrawOutcome>;
}

export type SubmitResult =
  | { taken: true; action: AddAction; via: 'answer' | 'withdraw' }
  | {
      taken: false;
      /** `unreadable`: the app answered in a shape this version cannot read, and then withdrew it (or could not be asked). */
      reason: 'refused' | 'no_answer' | 'unreadable';
      detail: string | null;
      /** The app may still have it (the withdraw failed), so it may end up in both places. */
      uncertain: boolean;
    };

/** Request failures after which the app may have the download, so `submit` withdraws it. */
const LOST_KINDS: ReadonlySet<RequestErrorKind> = new Set([
  'timeout',
  'disconnected',
  'unreadable',
]);

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
    // No answer, or one that cannot be read: the app may have it either way, so ask it to withdraw.
    const lost = error instanceof RequestError && LOST_KINDS.has(error.kind);
    if (!lost)
      return { taken: false, reason: 'refused', detail: messageOf(error), uncertain: false };
    const reason = error.kind === 'unreadable' ? 'unreadable' : 'no_answer';
    let outcome: WithdrawOutcome;
    try {
      outcome = await deps.withdraw(download.handover_id);
    } catch (withdrawError) {
      return { taken: false, reason, detail: messageOf(withdrawError), uncertain: true };
    }
    if (outcome === 'already_started') return { taken: true, action: 'started', via: 'withdraw' };
    return { taken: false, reason, detail: messageOf(error), uncertain: false };
  }
  // Every action the app answers with means it has the download; one this version does not
  // know never gets here, because the client fails such an answer as unreadable.
  return { taken: true, action: added.action, via: 'answer' };
}

export interface HandoverDeps extends SubmitDeps {
  downloads: DownloadControl;
  /**
   * The values to judge by after pausing: the app's live ones (connecting within `timeoutMs`
   * when needed), or the session copy when the app cannot be reached; null if neither.
   */
  liveSettings: (timeoutMs: number) => Promise<JudgingSettings | null>;
  /** Asks whether the app takes the capture, with no cookies; rejects when no answer comes within `timeoutMs` (D279). */
  check: (check: DownloadCheck, timeoutMs: number) => Promise<CheckAnswer>;
  /** The cookies the browser holds for a URL in this context, read for the sites the app named. Never rejects. */
  cookies: (
    url: string,
    store: CookieStore,
    request: CookieRequest,
    plan: CookiePlan,
  ) => Promise<DownloadCookies>;
  /** The private session the download comes from; null for a normal window. */
  privateSession: (incognito: boolean) => Promise<string | null>;
  /** When the wait for the app ends, in the clock of `now`: one deadline from the browser's event (D279). */
  deadline: number;
  now: () => number;
  userAgent: string | null;
}

/** Why the browser kept a download it was about to hand over. */
export type KeptReason =
  /** The browser would not pause it (usually because it already finished). */
  | 'pause_failed'
  /** The app could not be reached. */
  | 'app_unreachable'
  /** The settings say to leave it in the browser. */
  | SkipReason
  /** The app answered with an error. */
  | 'app_refused'
  /** The app said to leave it in the browser (D279). */
  | 'app_left'
  /** No answer came back, and the app then withdrew it (or could not be asked). */
  | 'no_answer'
  /** The app answered in a shape this version cannot read, and then withdrew it (or could not be asked). */
  | 'app_unreadable';

/** What happened to the browser's copy of a kept download. */
export type BrowserCopy =
  /** Never paused. */
  | 'untouched'
  | 'resumed'
  /** Still paused in the browser's list: the person has to resume it. */
  | 'stuck_paused'
  /** Resuming failed and the browser could not say how the copy stands: it may still be paused. */
  | 'maybe_paused'
  /** No longer running in the browser: finished, interrupted (cancelled or failed), or not listed. */
  | 'gone';

export type HandoverResult =
  | {
      kind: 'handed_over';
      handoverId: string;
      action: AddAction;
      browserCopy: CopyRemoval;
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
  /** The id the download is offered to the app under, recorded before pausing so a restarted background can withdraw it. */
  handoverId: string;
  candidate: CaptureCandidate;
  /** Null or absent when the browser's requests do not explain the download. */
  route?: DownloadRoute | null;
  cookieStore: CookieStore;
}

/**
 * Moves a browser download to the app without ever losing it:
 *
 * 1. Pause the browser download. If that fails, leave it alone.
 * 2. Check the settings again (the app's, fetched first when not known) and ask the app with
 *    `check_download`, which carries no cookies, whether it takes the download; one deadline from
 *    the browser's event bounds both (D279).
 * 3. Read the cookies for the sites the app named, and send `add_download` under the handover id the caller chose and saved before pausing; if
 *    no answer comes back, withdraw it (see `submit`).
 * 4. Only when the app has it (started, queued or awaiting confirmation): cancel and erase the browser copy,
 *    if it is still paused (see `removeBrowserCopy`).
 *    Only `started` means the app has reached the server and data is flowing. `queued` and
 *    `awaiting_confirmation` mean accepted but the link not yet tried, so some risk remains: a
 *    link that only works once, or one that needs the browser's POST request, may fail later in
 *    the app after the browser copy is gone. The app then shows the error with a retry. The
 *    extension's webRequest access only observes responses (to spot media); it does not look
 *    at download requests.
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

  const left = () => Math.max(0, deps.deadline - deps.now());
  const judging = await deps.liveSettings(left());
  if (judging === null) return keep(null, 'app_unreachable', null);
  const judged = await withLatestVerdict(candidate, deps.downloads, downloadId);
  const decision = shouldCapture(judged, judging.settings);
  if (!decision.capture) return keep(null, decision.reason, null);
  if (!judging.fromApp) return keep(null, 'app_unreachable', null);

  const asked = await askApp(candidate, deps, left);
  if ('reason' in asked) return keep(null, asked.reason, asked.detail);
  const { handoverId } = input;
  const download = newDownload(handoverId, deps.userAgent, {
    ...browserFields(input),
    browser_route: { ...browserRoute(input), private_session: asked.privateSession },
    ...(await deps.cookies(candidate.url, input.cookieStore, asked.request, asked.plan)),
  });
  const result = await submit(download, deps);
  if (!result.taken) {
    return keep(handoverId, KEPT_REASONS[result.reason], result.detail, result.uncertain);
  }

  const browserCopy = await removeBrowserCopy(deps.downloads, downloadId);
  return { kind: 'handed_over', handoverId, action: result.action, browserCopy };
}

/** A browser download is a top-level navigation started by the page it came from. */
function navigationOf(candidate: CaptureCandidate): CookieRequest {
  return { initiator: candidate.referrer, original_url: candidate.originalUrl, navigation: true };
}

/**
 * Asks the app with `check_download` whether it takes the download, with no cookies (D279): the
 * sites to read cookies for when it does, or why it stays in the browser. Nothing with credentials
 * was sent when the answer does not come, so there is nothing to withdraw.
 */
async function askApp(
  candidate: CaptureCandidate,
  deps: HandoverDeps,
  left: () => number,
): Promise<
  | { plan: CookiePlan; request: CookieRequest; privateSession: string | null }
  | { reason: KeptReason; detail: string | null }
> {
  const request = navigationOf(candidate);
  const privateSession = await deps.privateSession(candidate.incognito);
  // Its private window closed: the app would have no session to forget it with (D279).
  if (candidate.incognito && privateSession === null) {
    return { reason: 'private_window', detail: null };
  }
  let answer: CheckAnswer;
  try {
    answer = await deps.check(
      {
        url: candidate.url,
        page_url: candidate.referrer,
        frame_url: null,
        file_name: candidate.fileName,
        mime: candidate.mime,
        size: candidate.size,
        source: 'browser_download',
        private_session: privateSession,
        cookie_request: request,
      },
      left(),
    );
  } catch (error) {
    const unsent = error instanceof RequestError && error.kind === 'too_large';
    return { reason: unsent ? 'app_refused' : 'app_unreachable', detail: messageOf(error) };
  }
  if (answer.decision === 'leave') return { reason: 'app_left', detail: answer.reason };
  return { plan: answer.cookies, request, privateSession };
}

/** What the browser saw of a download, as the app is told it. */
function browserFields(
  input: HandoverInput,
): Omit<DownloadFields, 'browser_route' | 'cookies' | 'cookie_jar'> {
  const { candidate } = input;
  return {
    url: candidate.url,
    referrer: candidate.referrer,
    page_url: candidate.referrer,
    suggested_name: candidate.fileName,
    mime: candidate.mime,
    size_hint: candidate.size,
    source: 'browser_download',
  };
}

/** How the browser reached the download and its page. */
function browserRoute(input: HandoverInput): NonNullable<DownloadFields['browser_route']> {
  const { candidate, route } = input;
  return {
    private_window: candidate.incognito,
    observed_page: route?.pageUrl ?? null,
    page_redirect_urls: route?.pageRedirectUrls,
    redirect_urls:
      route?.redirectUrls ?? (candidate.originalUrl === null ? [] : [candidate.originalUrl]),
  };
}

/** Why the browser keeps a download the app did not take, by why the app did not take it. */
const KEPT_REASONS = {
  refused: 'app_refused',
  no_answer: 'no_answer',
  unreadable: 'app_unreadable',
} as const satisfies Record<Extract<SubmitResult, { taken: false }>['reason'], KeptReason>;

/** The browser may have flagged the file since it was first seen; its latest verdict counts. */
async function withLatestVerdict(
  candidate: CaptureCandidate,
  downloads: DownloadControl,
  id: number,
): Promise<CaptureCandidate> {
  const latest = await downloads
    .state(id)
    .catch(fallback('capture', 'Could not read the latest danger verdict of a download', null));
  const danger = nonEmpty(latest?.danger);
  return danger === null ? candidate : { ...candidate, danger };
}

/** What became of the browser's copy once the app has a download. */
export type CopyRemoval =
  /** Cancelled and taken off the browser's list, or already gone. */
  | 'removed'
  /** Read as paused, but could not be cancelled: it stays paused in the browser's list. */
  | 'left_paused'
  /**
   * The person resumed it, or it finished, while the app was answering, so it was left alone;
   * or it could not be read and the cancel failed, so it may be running; or it could not be read
   * after the cancel, so it may have finished first. Either way it may be in both places.
   */
  | 'kept_running';

/** The browser's view of a download, null when it no longer lists it, or `unreadable` when it would not say. */
export async function readBrowserCopy(
  downloads: DownloadControl,
  id: number,
): Promise<DownloadState | null | 'unreadable'> {
  try {
    return await downloads.state(id);
  } catch (error) {
    log.warn('capture', 'Could not read a browser download', error);
    return 'unreadable';
  }
}

/**
 * Cancels the browser's copy and takes it off the browser's list, but only while it is still
 * paused: the answer may have taken seconds, and a copy the person resumed meanwhile is theirs.
 * It is erased only once it reads as interrupted, since one that finished just before the
 * cancel is a whole file, and erasing it would only lose its history.
 */
export async function removeBrowserCopy(
  downloads: DownloadControl,
  id: number,
): Promise<CopyRemoval> {
  const before = await readBrowserCopy(downloads, id);
  const settled = beforeCancel(before);
  if (settled !== null) return settled;
  try {
    await downloads.cancel(id);
  } catch (error) {
    log.warn('capture', 'Could not cancel the browser copy of a download', error);
    // Only a copy read as paused is known to wait; one that would not say may be running.
    return before === 'unreadable' ? 'kept_running' : 'left_paused';
  }
  const after = afterCancel(await readBrowserCopy(downloads, id));
  if (after !== 'erase') return after;
  try {
    await downloads.erase(id);
  } catch (error) {
    // Cancelled but still listed: harmless, the person can clear it from the list.
    log.debug('capture', 'Could not erase a cancelled download from the list', error);
  }
  return 'removed';
}

/** What a copy read before the cancel comes to without one; null when it is to be cancelled (an unreadable one too). */
function beforeCancel(state: DownloadState | null | 'unreadable'): CopyRemoval | null {
  if (state === 'unreadable') return null;
  if (state === null || state.state === 'interrupted') return 'removed';
  return state.state === 'in_progress' && state.paused ? null : 'kept_running';
}

/** What a copy read after the cancel comes to; `erase` once it reads as cancelled. */
function afterCancel(state: DownloadState | null | 'unreadable'): CopyRemoval | 'erase' {
  if (state === null) return 'removed';
  // A copy that cannot be read may have finished before the cancel: warn of two copies.
  if (state === 'unreadable') return 'kept_running';
  if (state.state === 'interrupted') return 'erase';
  return state.state === 'in_progress' && state.paused ? 'left_paused' : 'kept_running';
}

/** Resumes a paused browser download; if that fails, finds out whether it is still waiting or already gone. */
export async function resumeBrowserCopy(
  downloads: DownloadControl,
  id: number,
): Promise<BrowserCopy> {
  try {
    await downloads.resume(id);
    return 'resumed';
  } catch (error) {
    log.warn('capture', 'Could not resume the browser copy of a download', error);
    const state = await readBrowserCopy(downloads, id);
    // A copy that cannot be read may still be paused, so the person is asked to look.
    if (state === 'unreadable') return 'maybe_paused';
    // Resuming a download that already runs fails in Firefox: the person resumed it.
    if (state?.state === 'in_progress') return state.paused ? 'stuck_paused' : 'resumed';
    return 'gone';
  }
}

/** The fields of a `NewDownload` that differ between captures; `newDownload` fills in the rest. */
export type DownloadFields = Omit<
  NewDownload,
  'handover_id' | 'user_agent' | 'extra_headers' | 'queue'
>;

/** A download to offer the app: the extension adds no headers of its own, and the app picks the queue. */
export function newDownload(
  handoverId: string,
  userAgent: string | null,
  fields: DownloadFields,
): NewDownload {
  return {
    ...fields,
    handover_id: handoverId,
    user_agent: userAgent,
    extra_headers: [],
    queue: null,
  };
}
