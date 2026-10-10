/**
 * The media button on web pages. Kept tiny: it runs in every page and frame, so it
 * only watches media elements and asks the background script before showing
 * anything. Nothing is sent anywhere until the person clicks the button.
 */
import { browser } from 'wxt/browser';
import { defineContentScript } from 'wxt/utils/define-content-script';
import { CACHE_KEY } from '@/lib/cache';
import { MediaButton } from '@/lib/media/button';
import { MEDIA_CHECK, MEDIA_DOWNLOAD, type DownloadAnswer } from '@/lib/media/messages';

/** The setting from the display cache the background keeps; on until the app says otherwise. */
function buttonSetting(cache: unknown): boolean {
  return (
    (cache as { settings?: { show_media_button?: unknown } } | undefined)?.settings?.show_media_button !==
    false
  );
}

function isAnswer(value: unknown): value is DownloadAnswer {
  const a = value as Partial<DownloadAnswer> | null;
  return typeof a?.ok === 'boolean' && typeof a.message === 'string';
}

export default defineContentScript({
  matches: ['http://*/*', 'https://*/*'],
  allFrames: true,
  runAt: 'document_idle',
  // Do not announce the script to the page with `window.postMessage`; some pages misread such messages.
  noScriptStartedPostMessage: true,
  main(ctx) {
    // Like downloads, media from private windows stays out of Wirecatch.
    if (browser.extension.inIncognitoContext) return;
    const button = new MediaButton({
      check: async (info) => {
        const answer: unknown = await browser.runtime.sendMessage({ type: MEDIA_CHECK, ...info });
        return (answer as { show?: unknown } | null)?.show === true;
      },
      download: async (request) => {
        const answer: unknown = await browser.runtime.sendMessage({ type: MEDIA_DOWNLOAD, ...request });
        return isAnswer(answer)
          ? answer
          : { ok: false, message: 'Wirecatch could not be reached. Open Wirecatch, then try again.' };
      },
    });
    // A reloaded or updated extension starts a fresh copy of this script: remove the old button.
    ctx.onInvalidated(() => button.destroy());
    browser.storage.local.get(CACHE_KEY).then(
      (items) => button.setEnabled(buttonSetting(items[CACHE_KEY])),
      () => button.setEnabled(true),
    );
    // Turning the setting on or off (in the popup or the app) takes effect at once.
    const onChanged = (changes: Record<string, { newValue?: unknown }>, area: string): void => {
      const change = changes[CACHE_KEY];
      if (area === 'local' && change !== undefined) button.setEnabled(buttonSetting(change.newValue));
    };
    browser.storage.onChanged.addListener(onChanged);
    ctx.onInvalidated(() => browser.storage.onChanged.removeListener(onChanged));
  },
});
