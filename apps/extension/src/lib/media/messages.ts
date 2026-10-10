/**
 * One-off messages between the media button (a content script) and the background
 * script. Kept apart from the popup messages so the content script stays tiny.
 */

export const MEDIA_CHECK = 'wirecatch:media-check';
export const MEDIA_DOWNLOAD = 'wirecatch:media-download';

/** What the content script knows about the element the button is over. */
export interface ElementInfo {
  /** The element's `currentSrc`, or null when it has none. */
  src: string | null;
  audio: boolean;
}

export type ContentRequest =
  /** Is there anything to download for this element? Asked before the button is shown. */
  | ({ type: typeof MEDIA_CHECK } & ElementInfo)
  /** The button was clicked. */
  | ({ type: typeof MEDIA_DOWNLOAD; title: string | null } & ElementInfo);

export interface CheckAnswer {
  show: boolean;
}

export interface DownloadAnswer {
  ok: boolean;
  /** Shown next to the button, for example "Sent to Wirecatch." or "Open Wirecatch, then try again." */
  message: string;
}

/** Checks the shape of a message from a content script; null for anything else. */
export function asContentRequest(raw: unknown): ContentRequest | null {
  if (typeof raw !== 'object' || raw === null) return null;
  const m = raw as Record<string, unknown>;
  if (m.type !== MEDIA_CHECK && m.type !== MEDIA_DOWNLOAD) return null;
  if (!(m.src === null || typeof m.src === 'string') || typeof m.audio !== 'boolean') return null;
  if (m.type === MEDIA_DOWNLOAD && !(m.title === null || typeof m.title === 'string')) return null;
  return raw as ContentRequest;
}
