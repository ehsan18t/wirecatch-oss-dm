import { log } from '../diag/log';
import type { Alert } from '../popup-port';
import type { MediaSendResult } from './center';
import { MEDIA_ANNOUNCE, type AnnounceMessage } from './messages';

export interface AnnounceDeps {
  /** `tabs.sendMessage` to one frame of a tab. */
  sendMessage: (
    tabId: number,
    message: AnnounceMessage,
    options: { frameId: number },
  ) => Promise<unknown>;
  /** An alert in the popup. */
  notify: (tone: Alert['tone'], message: string) => void;
}

/**
 * Shows the answer to a media download sent from the keyboard. The page's top frame shows it
 * next to where the person is looking. On pages without the content script (browser pages, other
 * extensions) a failure goes to the popup; a success needs no alert, since the app shows the download.
 */
export function announcer(deps: AnnounceDeps): (tabId: number, result: MediaSendResult) => void {
  return (tabId, result) => {
    const message: AnnounceMessage = {
      type: MEDIA_ANNOUNCE,
      ok: result.ok,
      message: result.message,
    };
    deps.sendMessage(tabId, message, { frameId: 0 }).catch((error: unknown) => {
      log.debug('media', 'The page cannot show the answer', error);
      if (!result.ok) deps.notify('error', result.message);
    });
  };
}
