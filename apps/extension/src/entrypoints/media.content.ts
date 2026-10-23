/**
 * The media button on web pages. Kept tiny: it runs in every page and frame, so it
 * only watches media elements and asks the background script before showing
 * anything. Nothing is sent anywhere until the person clicks the button.
 */
import { browser } from 'wxt/browser';
import { defineContentScript } from 'wxt/utils/define-content-script';
import { log } from '@/lib/diag/log';
import { MediaButton } from '@/lib/media/button';
import { SITE_ORIGINS } from '@/lib/site-access';
import {
  asAnnounce,
  MEDIA_BUTTON_QUERY,
  MEDIA_CHECK,
  MEDIA_DOWNLOAD,
  frameLocation,
  isButtonSetting,
  isCheckAnswer,
  isDownloadAnswer,
  type ContentRequest,
} from '@/lib/media/messages';

export default defineContentScript({
  matches: SITE_ORIGINS,
  allFrames: true,
  runAt: 'document_idle',
  // Do not announce the script to the page with `window.postMessage`; some pages misread such messages.
  noScriptStartedPostMessage: true,
  main(ctx) {
    // Like downloads, media from private windows stays out of Wirecatch.
    if (browser.extension.inIncognitoContext) return;
    // Every question carries the frame's address as it is now: a single-page site changes it without loading a new page, and the browser keeps reporting the address this script started on.
    const ask = (request: ContentRequest): Promise<unknown> => browser.runtime.sendMessage(request);
    const button = new MediaButton({
      check: async (info) => {
        const answer = await ask({
          type: MEDIA_CHECK,
          ...info,
          location: frameLocation(location.href),
        });
        return isCheckAnswer(answer) && answer.show;
      },
      download: async (request) => {
        const answer = await ask({
          type: MEDIA_DOWNLOAD,
          ...request,
          location: frameLocation(location.href),
        });
        return isDownloadAnswer(answer)
          ? answer
          : {
              ok: false,
              message: 'Wirecatch could not be reached. Open Wirecatch, then try again.',
            };
      },
    });
    // A reloaded or updated extension starts a fresh copy of this script: remove the old button.
    ctx.onInvalidated(() => button.destroy());
    // The app keeps the setting (D61); the background says whether it is on, and on unless it can say.
    ask({ type: MEDIA_BUTTON_QUERY }).then(
      (answer) => button.setEnabled(!isButtonSetting(answer) || answer.on),
      (error: unknown) => {
        log.warn('page', 'Could not ask for the media button setting; showing the button', error);
        button.setEnabled(true);
      },
    );
    // From the background (pages cannot send these): the setting turned on or off, in the popup
    // or the app, which takes effect at once; and the answer to the keyboard shortcut.
    const onMessage = (raw: unknown): undefined => {
      if (isButtonSetting(raw)) {
        button.setEnabled(raw.on);
        return undefined;
      }
      const announce = asAnnounce(raw);
      if (announce !== null) button.announce(announce.message, !announce.ok);
      return undefined;
    };
    browser.runtime.onMessage.addListener(onMessage);
    ctx.onInvalidated(() => browser.runtime.onMessage.removeListener(onMessage));
  },
});
