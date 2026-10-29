/**
 * One-off messages between the media button (a content script) and the background
 * script. Kept apart from the popup messages so the content script stays tiny.
 */
import { KB } from '@wirecatch/units';
import {
  fields,
  isBoolean,
  isString,
  literal,
  nullable,
  object,
  optional,
  refine,
} from '../validate';

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

const isAnnounce = fields<AnnounceMessage>({
  type: literal(MEDIA_ANNOUNCE),
  ok: isBoolean,
  message: isString,
});

export function asAnnounce(raw: unknown): AnnounceMessage | null {
  if (!isAnnounce(raw)) return null;
  return { type: MEDIA_ANNOUNCE, ok: raw.ok, message: raw.message.slice(0, MAX_TITLE_LENGTH) };
}

/** Asked by the media button as its page loads: whether "Show download button on videos" is on. Answered with a `ButtonSetting`. */
export const MEDIA_BUTTON_QUERY = 'wirecatch:media-button-query';

/** Sent by the background to every open page when "Show download button on videos" changes, and the answer to `MEDIA_BUTTON_QUERY`. A content script learns this setting alone, never the others (D61). */
export const MEDIA_BUTTON_SETTING = 'wirecatch:media-button-setting';

export interface ButtonSetting {
  type: typeof MEDIA_BUTTON_SETTING;
  on: boolean;
}

export const isButtonSetting = fields<ButtonSetting>({
  type: literal(MEDIA_BUTTON_SETTING),
  on: isBoolean,
});

/**
 * Longest media address handled (8 KB), whether a page sends it or it is seen on the
 * network. Real media addresses are far shorter; longer ones are not worth handing
 * over and would only fill the session storage the lists are saved to.
 */
export const MAX_MEDIA_URL_LENGTH = 8 * KB;
/**
 * Longest title a page sends, and longest answer shown on a page, in characters. Real page and
 * media titles are far shorter (browsers show about a hundred); the cap only keeps a page from
 * pushing long text through to the app or the alert, so its exact value matters little.
 */
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
  | ({ type: typeof MEDIA_DOWNLOAD; title: string | null } & ElementInfo & FrameInfo)
  /** Is the button on at all? */
  | { type: typeof MEDIA_BUTTON_QUERY };

/** A request about one media element: a check or a click. */
export type ElementRequest = Exclude<ContentRequest, { type: typeof MEDIA_BUTTON_QUERY }>;

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

const isSrc = nullable(refine(isString, isWebSrc));

const isTitle = nullable(refine(isString, (title) => title.length <= MAX_TITLE_LENGTH));

/** The fields both content script requests carry; `location` is missing in messages from older content scripts. */
const elementFields = { src: isSrc, audio: isBoolean, location: optional(isSrc) };

const isCheck = object({ type: literal(MEDIA_CHECK), ...elementFields });

const isDownload = object({ type: literal(MEDIA_DOWNLOAD), title: isTitle, ...elementFields });

const isButtonQuery = object({ type: literal(MEDIA_BUTTON_QUERY) });

/**
 * Checks the shape of a message from a content script; null for anything else. A missing `location` reads as null (the background then uses the browser's record of the frame's address).
 */
export function asContentRequest(raw: unknown): ContentRequest | null {
  if (isCheck(raw)) {
    return { type: MEDIA_CHECK, src: raw.src, audio: raw.audio, location: raw.location ?? null };
  }
  if (isDownload(raw)) {
    const { title, src, audio } = raw;
    return { type: MEDIA_DOWNLOAD, title, src, audio, location: raw.location ?? null };
  }
  if (isButtonQuery(raw)) return { type: MEDIA_BUTTON_QUERY };
  return null;
}

/** The background's answer to `MEDIA_CHECK`. */
export const isCheckAnswer = fields<CheckAnswer>({ show: isBoolean });

/** The background's answer to `MEDIA_DOWNLOAD`. */
export const isDownloadAnswer = fields<DownloadAnswer>({ ok: isBoolean, message: isString });
