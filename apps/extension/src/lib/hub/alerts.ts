/** The sentences the hub shows the person, in the popup and next to the media button. */
import { PRODUCT_NAME } from '@wirecatch/product';
import type { HandoverResult, SubmitResult } from '../capture/handover';
import { messageOf } from '../errors';
import type { MediaSendResult } from '../media/center';
import { RequestError, UNREADABLE_ANSWER } from '../native/client';

export const NO_SITE_ACCESS = `${PRODUCT_NAME} needs access to websites to capture downloads with their cookies. Open the ${PRODUCT_NAME} popup and click Allow access.`;

export const MEDIA_NO_SITE_ACCESS = `${PRODUCT_NAME} needs access to websites to send media with their cookies. Open the ${PRODUCT_NAME} popup and click Allow access.`;

export const MENU_PRIVATE_WINDOW = `${PRODUCT_NAME} is turned off in private windows. Turn on "Work in private windows" in the ${PRODUCT_NAME} popup, or use the browser's own "Save as" option.`;

export const MENU_NOT_WEB = `${PRODUCT_NAME} can only capture web links, so the browser is downloading this item instead.`;

export const BROWSER_DOWNLOAD_FAILED =
  'This item could not be downloaded. Try the browser\'s own "Save as" option instead.';

/** A popup command's error, as the popup shows it. */
export function describeError(error: unknown): string {
  if (error instanceof RequestError) {
    if (error.kind === 'timeout') return `${PRODUCT_NAME} did not answer in time.`;
    return error.message;
  }
  return messageOf(error);
}

export function copyLeftInBrowser(name: string): string {
  return `${name} went to ${PRODUCT_NAME}, but its paused copy is still in the browser's download list. You can remove it there.`;
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
    ? `${paused}, and ${PRODUCT_NAME} may have taken it. Check ${PRODUCT_NAME}'s download list: if it is not there, ${resume}`
    : `${paused}. ${resume.charAt(0).toUpperCase()}${resume.slice(1)}`;
}

/** The browser has a copy and the app could not say whether it has one too. */
export function mayDownloadTwiceUncertain(name: string): string {
  return `${name} may be downloading twice: ${PRODUCT_NAME} may have taken it, and the browser is downloading it too. If ${PRODUCT_NAME} has it, cancel one of them.`;
}

export function mayDownloadTwice(name: string): string {
  return `${name} may be downloading twice: ${PRODUCT_NAME} took it after all, and the browser kept its copy too. You can cancel one of them.`;
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
  if (result.reason === 'app_unreachable') return `${PRODUCT_NAME} could not be reached`;
  if (result.reason === 'no_answer') return `${PRODUCT_NAME} did not answer in time`;
  if (result.reason === 'app_unreadable') return UNREADABLE_ANSWER;
  if (result.reason === 'app_refused') return result.detail ?? `${PRODUCT_NAME} did not accept it`;
  return null;
}

/** The context menu's link went to the browser instead, after the app did not take it. */
export function menuNotTaken(name: string, detail: string | null): string {
  const reason = detail === null ? '' : ` ${detail}`;
  return `${PRODUCT_NAME} could not capture ${name}, so the browser is downloading it instead.${reason}`;
}

/** The answer shown next to the media button or in the popup's media list. */
export function mediaAnswer(result: SubmitResult): MediaSendResult {
  if (result.taken) {
    return {
      ok: true,
      message:
        result.action === 'awaiting_confirmation'
          ? `Sent to ${PRODUCT_NAME}. Confirm it there to start.`
          : `Sent to ${PRODUCT_NAME}.`,
    };
  }
  if (result.reason === 'refused') {
    const why = result.detail === null ? '' : ` ${result.detail.replace(/\.?$/, '.')}`;
    return { ok: false, message: `${PRODUCT_NAME} did not accept it.${why}` };
  }
  if (result.reason === 'unreadable') {
    return {
      ok: false,
      message: result.uncertain
        ? `${UNREADABLE_ANSWER} Open ${PRODUCT_NAME} and check its list before adding it again.`
        : `Nothing was added. ${UNREADABLE_ANSWER}`,
    };
  }
  return {
    ok: false,
    message: result.uncertain
      ? `${PRODUCT_NAME} did not answer in time. Open ${PRODUCT_NAME} and check its list before trying again.`
      : `${PRODUCT_NAME} did not answer in time, so nothing was added. Open ${PRODUCT_NAME}, then try again.`,
  };
}
