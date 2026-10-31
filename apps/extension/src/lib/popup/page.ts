// What the extension's own page, the popup, does as it opens: connect to the
// background script, and read what the browser knows that the background does not.
import { browser } from 'wxt/browser';
import { fallback, log } from '../diag/log';
import { MEDIA_COMMAND } from '../media/messages';
import { POPUP_PORT_NAME, type PopupRequest } from '../popup-port';
import { SITE_ORIGINS } from '../site-access';
import { nonEmpty } from '../text';
import { PopupModel } from './model.svelte';

/** Longest a page waits for its first state before showing itself anyway. */
const FIRST_STATE_WAIT_MS = 300;

/** Connects to the background script; the model fills in as its messages arrive. */
export function connectPage(): PopupModel {
  const port = browser.runtime.connect({ name: POPUP_PORT_NAME });
  return new PopupModel({
    postMessage: (message: PopupRequest) => {
      port.postMessage(message);
    },
    onMessage: port.onMessage,
    onDisconnect: port.onDisconnect,
  });
}

/** Waits for what the page shows first (`PopupModel.loaded`), but never longer than a moment. */
export function firstState(model: PopupModel, media: boolean): Promise<void> {
  return new Promise((resolve) => {
    const timer = setTimeout(resolve, FIRST_STATE_WAIT_MS);
    void model.loaded(media).then(() => {
      clearTimeout(timer);
      resolve();
    });
  });
}

/** The tab the popup was opened over. */
export async function activeTab(): Promise<{ id: number | null; url: string | null }> {
  try {
    const [tab] = await browser.tabs.query({ active: true, currentWindow: true });
    return { id: tab?.id ?? null, url: tab?.url ?? null };
  } catch (error) {
    log.warn('popup', 'Could not read the active tab', error);
    return { id: null, url: null };
  }
}

/** The keyboard shortcut for downloading a page's media, as the browser shows it; null when none is set. */
export async function mediaShortcut(): Promise<string | null> {
  try {
    const commands = await browser.commands.getAll();
    return nonEmpty(commands.find((c) => c.name === MEDIA_COMMAND)?.shortcut);
  } catch (error) {
    log.warn('popup', 'Could not read the keyboard shortcuts', error);
    return null;
  }
}

/** False when the person revoked the extension's access to websites (possible in Firefox). */
export function hasSiteAccess(): Promise<boolean> {
  return browser.permissions
    .contains({ origins: SITE_ORIGINS })
    .catch(fallback('popup', 'Could not check access to websites', true));
}

/** Must run from the click itself: browsers only show the permission prompt for a user action. */
export function requestSiteAccess(): Promise<boolean> {
  return browser.permissions
    .request({ origins: SITE_ORIGINS })
    .catch(fallback('popup', 'Could not ask for access to websites', false, 'error'));
}
