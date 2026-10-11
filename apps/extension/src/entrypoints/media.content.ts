/**
 * The media button on web pages. Kept tiny: it runs in every page and frame, so it
 * only watches media elements and asks the background script before showing
 * anything. Nothing is sent anywhere until the person clicks the button.
 */
import { browser } from 'wxt/browser';
import { defineContentScript } from 'wxt/utils/define-content-script';
import { MediaButton } from '@/lib/media/button';
import { SITE_ORIGINS } from '@/lib/site-access';
import {
  MEDIA_BUTTON_KEY,
  asAnnounce,
  MEDIA_CHECK,
  MEDIA_DOWNLOAD,
  frameLocation,
  mediaButtonOn,
  type ContentRequest,
  type DownloadAnswer,
} from '@/lib/media/messages';

function isAnswer(value: unknown): value is DownloadAnswer {
  const a = value as Partial<DownloadAnswer> | null;
  return typeof a?.ok === 'boolean' && typeof a.message === 'string';
}

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
        return (answer as { show?: unknown } | null)?.show === true;
      },
      download: async (request) => {
        const answer = await ask({
          type: MEDIA_DOWNLOAD,
          ...request,
          location: frameLocation(location.href),
        });
        return isAnswer(answer)
          ? answer
          : {
              ok: false,
              message: 'Wirecatch could not be reached. Open Wirecatch, then try again.',
            };
      },
    });
    // A reloaded or updated extension starts a fresh copy of this script: remove the old button.
    ctx.onInvalidated(() => button.destroy());
    browser.storage.local.get(MEDIA_BUTTON_KEY).then(
      (items) => button.setEnabled(mediaButtonOn(items[MEDIA_BUTTON_KEY])),
      () => button.setEnabled(true),
    );
    // Turning the setting on or off (in the popup or the app) takes effect at once.
    const onChanged = (changes: Record<string, { newValue?: unknown }>, area: string): void => {
      const change = changes[MEDIA_BUTTON_KEY];
      if (area === 'local' && change !== undefined)
        button.setEnabled(mediaButtonOn(change.newValue));
    };
    browser.storage.onChanged.addListener(onChanged);
    ctx.onInvalidated(() => browser.storage.onChanged.removeListener(onChanged));
    // The answer to the keyboard shortcut, from the background (pages cannot send these).
    const onMessage = (raw: unknown): undefined => {
      const announce = asAnnounce(raw);
      if (announce !== null) button.announce(announce.message, !announce.ok);
      return undefined;
    };
    browser.runtime.onMessage.addListener(onMessage);
    ctx.onInvalidated(() => browser.runtime.onMessage.removeListener(onMessage));
  },
});
