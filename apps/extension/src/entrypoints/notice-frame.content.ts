/**
 * Places the notices frame in the page when the background asks (D65): the app's notices about
 * downloads, in the tab the person is looking at. Kept tiny, since it runs in every page: it
 * does nothing until asked, and the notices themselves never pass through it.
 */
import { browser } from 'wxt/browser';
import { defineContentScript } from 'wxt/utils/define-content-script';
import {
  NOTICE_MOUNT,
  NOTICE_UNMOUNT,
  NOTICES_PAGE,
  type MountAnswer,
} from '@/lib/notices/messages';
import { NoticesFrame } from '@/lib/notices/frame';
import { SITE_ORIGINS } from '@/lib/site-access';
import { fields, literal } from '@/lib/validate';

const isMount = fields({ type: literal(NOTICE_MOUNT) });
const isUnmount = fields({ type: literal(NOTICE_UNMOUNT) });

export default defineContentScript({
  matches: SITE_ORIGINS,
  runAt: 'document_idle',
  noScriptStartedPostMessage: true,
  main(ctx) {
    const frame = new NoticesFrame({
      container: () => document.fullscreenElement ?? document.documentElement,
      noticesUrl: (token) => `${browser.runtime.getURL(NOTICES_PAGE)}#${token}`,
      extensionOrigin: new URL(browser.runtime.getURL('/')).origin,
      newToken: () => crypto.randomUUID(),
    });
    // From the background only: pages cannot send these. Answered through `sendResponse`, which both browsers support.
    const onMessage = (
      raw: unknown,
      _sender: unknown,
      sendResponse: (answer: MountAnswer) => void,
    ): undefined => {
      if (isMount(raw)) {
        // No answer when the page cannot take notices: the background gives it none.
        const token = frame.mount();
        if (token !== null) sendResponse({ token });
      } else if (isUnmount(raw)) {
        frame.unmount();
      }
      return undefined;
    };
    browser.runtime.onMessage.addListener(onMessage);
    // Going in or out of full screen moves the host where it shows; it says nothing about notices.
    const onFullscreen = () => frame.place();
    document.addEventListener('fullscreenchange', onFullscreen);
    ctx.onInvalidated(() => {
      browser.runtime.onMessage.removeListener(onMessage);
      document.removeEventListener('fullscreenchange', onFullscreen);
      frame.destroy();
    });
  },
});
