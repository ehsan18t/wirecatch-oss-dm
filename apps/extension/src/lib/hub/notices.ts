/** The sentences the hub shows the person, in the popup and next to the media button. */
import type { HandoverResult, SubmitResult } from '../capture/handover';
import type { MediaSendResult } from '../media/center';
import { RequestError, UNREADABLE_ANSWER } from '../native/client';

export const NO_SITE_ACCESS =
  'Wirecatch needs access to websites to take downloads with their cookies. Open this popup and click Allow access.';

/** For media there is no browser download to fall back on, so the person has to act. */
export const MEDIA_UNREACHABLE = 'Wirecatch could not be reached. Open Wirecatch, then try again.';

export const MEDIA_NO_SITE_ACCESS =
  'Wirecatch needs access to websites to send media with its cookies. Open the Wirecatch popup and click Allow access.';

export const MENU_PRIVATE_WINDOW =
  'Wirecatch does not take downloads from private windows. Use the browser\'s own "Save as" option there.';

export const BROWSER_DOWNLOAD_FAILED =
  'This item could not be downloaded. Try the browser\'s own "Save as" option instead.';

/** A popup command's error, as the popup shows it. */
export function describeError(error: unknown): string {
  if (error instanceof RequestError) {
    if (error.kind === 'timeout') return 'Wirecatch did not answer in time.';
    return error.message;
  }
  return error instanceof Error ? error.message : String(error);
}

export function copyLeftInBrowser(name: string): string {
  return `${name} went to Wirecatch, but its paused copy is still in the browser's download list. You can remove it there.`;
}

/**
 * The browser would not resume its copy. `certain` is false when the browser could not say how
 * the copy stands; `appMayHave` when the app could not say whether it took the download.
 */
export function stuckPaused(
  name: string,
  { certain, appMayHave }: { certain: boolean; appMayHave: boolean },
): string {
  const paused = `${name} ${certain ? 'is' : 'may be'} paused in the browser`;
  const resume = "open the browser's downloads page and click Resume.";
  return appMayHave
    ? `${paused}, and Wirecatch may have taken it. Check Wirecatch's download list: if it is not there, ${resume}`
    : `${paused}. ${resume.charAt(0).toUpperCase()}${resume.slice(1)}`;
}

/** The browser has a copy and the app could not say whether it has one too. */
export function mayDownloadTwiceUncertain(name: string): string {
  return `${name} may be downloading twice: Wirecatch may have taken it, and the browser is downloading it too. If Wirecatch has it, cancel one of them.`;
}

export function mayDownloadTwice(name: string): string {
  return `${name} may be downloading twice: Wirecatch took it after all, and the browser kept its copy too. You can cancel one of them.`;
}

/** Why a download stayed in the browser, for the reasons worth telling; null for the others. */
export function keptInBrowser(
  result: Extract<HandoverResult, { kind: 'kept' }>,
  name: string,
): string | null {
  const why = keptReason(result);
  return why === null ? null : `${name} stayed in the browser: ${why.replace(/\.$/, '')}.`;
}

function keptReason(result: Extract<HandoverResult, { kind: 'kept' }>): string | null {
  // The other reasons are the settings or a download the browser would not pause: nothing to tell.
  if (result.reason === 'app_unreachable') return 'Wirecatch could not be reached';
  if (result.reason === 'no_answer') return 'Wirecatch did not answer in time';
  if (result.reason === 'app_unreadable') return UNREADABLE_ANSWER;
  if (result.reason === 'app_refused') return result.detail ?? 'Wirecatch did not accept it';
  return null;
}

/** The context menu's link went to the browser instead, after the app did not take it. */
export function menuNotTaken(name: string, detail: string | null): string {
  const reason = detail === null ? '' : ` ${detail}`;
  return `Wirecatch could not take ${name}, so the browser is downloading it instead.${reason}`;
}

/** The answer shown next to the media button or in the popup's media list. */
export function mediaAnswer(result: SubmitResult): MediaSendResult {
  if (result.taken) {
    return {
      ok: true,
      message:
        result.action === 'awaiting_confirmation'
          ? 'Sent to Wirecatch. Confirm it there to start.'
          : 'Sent to Wirecatch.',
    };
  }
  if (result.reason === 'refused') {
    const why = result.detail === null ? '' : ` ${result.detail.replace(/\.?$/, '.')}`;
    return { ok: false, message: `Wirecatch did not take it.${why}` };
  }
  if (result.reason === 'unreadable') {
    return {
      ok: false,
      message: result.uncertain
        ? `${UNREADABLE_ANSWER} Open Wirecatch and check its list before adding it again.`
        : `Nothing was added. ${UNREADABLE_ANSWER}`,
    };
  }
  return {
    ok: false,
    message: result.uncertain
      ? 'Wirecatch did not answer in time. Open Wirecatch and check its list before trying again.'
      : 'Wirecatch did not answer in time, so nothing was added. Open Wirecatch, then try again.',
  };
}
