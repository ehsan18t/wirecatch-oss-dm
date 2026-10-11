/**
 * One-off messages between the media button (a content script) and the background
 * script. Kept apart from the popup messages so the content script stays tiny.
 */

export const MEDIA_CHECK = 'wirecatch:media-check';

/** Name of the keyboard shortcut (`commands` in the manifest) that downloads a page's media. */
export const MEDIA_COMMAND = 'download-media';

/** Sent by the background to the page's top frame: show this answer to the shortcut. */
export const MEDIA_ANNOUNCE = 'wirecatch:media-announce';

export interface AnnounceMessage {
  type: typeof MEDIA_ANNOUNCE;
  ok: boolean;
  message: string;
}

export function asAnnounce(raw: unknown): AnnounceMessage | null {
  const m = raw as Partial<AnnounceMessage> | null;
  if (m?.type !== MEDIA_ANNOUNCE || typeof m.ok !== 'boolean' || typeof m.message !== 'string')
    return null;
  return { type: MEDIA_ANNOUNCE, ok: m.ok, message: m.message.slice(0, MAX_TITLE_LENGTH) };
}

/**
 * `storage.local` key holding only the "Show download button on videos" setting, as
 * a boolean. Content scripts read this key alone, never the settings cache with the
 * list of ignored sites.
 */
export const MEDIA_BUTTON_KEY = 'wirecatch.mediaButton.v1';

/** Reads the stored setting: on unless it is explicitly off (the app's default is on). */
export function mediaButtonOn(stored: unknown): boolean {
  return stored !== false;
}
/**
 * Longest media address handled (8 KiB), whether a page sends it or it is seen on the
 * network. Real media addresses are far shorter; longer ones are not worth handing
 * over and would only fill the session storage the lists are saved to.
 */
export const MAX_MEDIA_URL_LENGTH = 8 * 1024;
/** Longest title sent from a page. */
export const MAX_TITLE_LENGTH = 1024;
export const MEDIA_DOWNLOAD = 'wirecatch:media-download';

/** What the content script knows about the element the button is over. */
export interface ElementInfo {
  /** The element's `currentSrc`, or null when it has none. */
  src: string | null;
  audio: boolean;
}

/** Where the content script runs, sent with every question so the answer is about the page shown now. */
export interface FrameInfo {
  /**
   * The frame's address now (`location.href`), or null when it is not a short web address.
   *
   * The browser's own record of the sender's address (`MessageSender.url`) is the address the frame had when the content script started, and a single-page site changes the address with the history API without loading a new page. The background trusts this claim only on the origin the browser reports.
   */
  location: string | null;
}

export type ContentRequest =
  /** Is there anything to download for this element? Asked before the button is shown. */
  | ({ type: typeof MEDIA_CHECK } & ElementInfo & FrameInfo)
  /** The button was clicked. */
  | ({ type: typeof MEDIA_DOWNLOAD; title: string | null } & ElementInfo & FrameInfo);

export interface CheckAnswer {
  show: boolean;
}

export interface DownloadAnswer {
  ok: boolean;
  /** Shown next to the button, for example "Sent to Wirecatch." or "Open Wirecatch, then try again." */
  message: string;
}

/** An http(s) address no longer than `MAX_MEDIA_URL_LENGTH`: the only sources worth sending. */
export function isWebSrc(src: string): boolean {
  return src.length <= MAX_MEDIA_URL_LENGTH && /^https?:\/\//i.test(src);
}

/** The frame's address as the content script sends it: null unless it is a short web address. */
export function frameLocation(href: string): string | null {
  return isWebSrc(href) ? href : null;
}

/**
 * Checks the shape of a message from a content script; null for anything else. A missing `location` reads as null (the background then uses the browser's record of the frame's address).
 */
export function asContentRequest(raw: unknown): ContentRequest | null {
  if (typeof raw !== 'object' || raw === null) return null;
  const m = raw as Record<string, unknown>;
  if (m.type !== MEDIA_CHECK && m.type !== MEDIA_DOWNLOAD) return null;
  const { src, audio, title } = m;
  const location = m.location ?? null;
  if (typeof audio !== 'boolean') return null;
  if (src !== null && !(typeof src === 'string' && isWebSrc(src))) return null;
  if (location !== null && !(typeof location === 'string' && isWebSrc(location))) return null;
  const info = { src, audio, location };
  if (m.type === MEDIA_CHECK) return { type: MEDIA_CHECK, ...info };
  if (!(title === null || (typeof title === 'string' && title.length <= MAX_TITLE_LENGTH)))
    return null;
  return { type: MEDIA_DOWNLOAD, title, ...info };
}
