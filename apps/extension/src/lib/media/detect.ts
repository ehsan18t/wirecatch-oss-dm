/**
 * Rules for spotting media in network responses the browser reports through
 * `webRequest`. Observing only: nothing here can change, delay or block a request.
 */

/** How detected media is delivered (the page kind is never detected from traffic). */
export type DetectedKind = 'hls' | 'dash' | 'direct';

/** One response as `webRequest.onResponseStarted` reports it, in both Chromium and Firefox. */
export interface ObservedResponse {
  url: string;
  /** -1 when the request does not belong to a tab (service workers, the browser itself, other extensions). */
  tabId: number;
  /** Resource type: `main_frame`, `media`, `xmlhttprequest` and so on. */
  type: string;
  statusCode: number;
  responseHeaders?: Array<{ name: string; value?: string }>;
  /** Firefox only: true for requests from private windows. Chromium leaves it out. */
  incognito?: boolean;
  /** Chromium only: `prerender` for pages loading in the background before anyone opens them. */
  documentLifecycle?: string;
  /** The frame that made the request: 0 for the tab's page, another number for a frame inside it. */
  frameId?: number;
}

/** Media found in one response. */
export interface Detection {
  kind: DetectedKind;
  url: string;
  mime: string | null;
  /** Total size in bytes when the server said; null for streams and unknown sizes. */
  size: number | null;
  /** True for audio-only direct files, so they can be labelled as such. */
  audio: boolean;
}

const HLS_TYPES = new Set([
  'application/vnd.apple.mpegurl',
  'application/x-mpegurl',
  'audio/mpegurl',
  'audio/x-mpegurl',
]);
const DASH_TYPES = new Set(['application/dash+xml', 'video/vnd.mpeg.dash.mpd']);

/** Pieces of a stream rather than whole files: listing them would only add noise. */
const SEGMENT_TYPES = new Set(['video/mp2t', 'video/iso.segment', 'audio/iso.segment']);
const SEGMENT_EXTENSIONS = new Set(['ts', 'm4s', 'm4f', 'm4v-seg', 'cmfv', 'cmfa', 'aac-seg']);

const VIDEO_EXTENSIONS = new Set(['mp4', 'm4v', 'webm', 'mkv', 'mov', 'ogv', 'avi', 'flv', '3gp']);
const AUDIO_EXTENSIONS = new Set(['mp3', 'm4a', 'ogg', 'oga', 'opus', 'wav', 'flac', 'aac', 'weba']);

/**
 * Resource types whose video or audio responses are whole files. Media fetched by
 * scripts (`xmlhttprequest`, which includes `fetch`) is almost always a slice fed to
 * a player through Media Source Extensions, so it is not listed as a file.
 */
const FILE_REQUEST_TYPES = new Set(['media', 'main_frame', 'sub_frame', 'object']);

/**
 * Longest address kept (8 KiB). Real media addresses are far shorter; longer ones
 * would only fill the session storage the lists are saved to.
 */
export const MAX_MEDIA_URL_LENGTH = 8 * 1024;

/** Longest media type kept; RFC 6838 allows 127 characters for each half. */
const MAX_MIME_LENGTH = 255;

/** True for http and https addresses; false for `blob:`, `data:`, `mediasource:` and anything else. */
export function isWebUrl(url: string): boolean {
  return /^https?:\/\//i.test(url);
}

/** The lower-case extension of the last path segment, or null. */
export function extensionOf(url: string): string | null {
  let path: string;
  try {
    path = new URL(url).pathname;
  } catch {
    return null;
  }
  const last = path.slice(path.lastIndexOf('/') + 1);
  const dot = last.lastIndexOf('.');
  if (dot <= 0 || dot === last.length - 1) return null;
  return last.slice(dot + 1).toLowerCase();
}

/** What an address alone says about the media it points to (by extension), or null. */
export function classifyUrl(url: string): { kind: DetectedKind; audio: boolean } | null {
  if (!isWebUrl(url)) return null;
  const ext = extensionOf(url);
  if (ext === null) return null;
  if (ext === 'm3u8') return { kind: 'hls', audio: false };
  if (ext === 'mpd') return { kind: 'dash', audio: false };
  if (VIDEO_EXTENSIONS.has(ext)) return { kind: 'direct', audio: false };
  if (AUDIO_EXTENSIONS.has(ext)) return { kind: 'direct', audio: true };
  return null;
}

function header(response: ObservedResponse, name: string): string | null {
  for (const h of response.responseHeaders ?? []) {
    if (h.name.toLowerCase() === name) return h.value ?? null;
  }
  return null;
}

/** The media type without parameters, lower case (`video/mp4; codecs=...` becomes `video/mp4`). */
function mimeOf(response: ObservedResponse): string | null {
  const raw = header(response, 'content-type');
  if (raw === null) return null;
  const mime = raw.split(';', 1)[0]?.trim().toLowerCase() ?? '';
  return mime === '' || mime.length > MAX_MIME_LENGTH ? null : mime;
}

/** Total size: the part after the slash in `Content-Range` for 206 answers, `Content-Length` for 200. */
function sizeOf(response: ObservedResponse): number | null {
  if (response.statusCode === 206) {
    const range = header(response, 'content-range');
    const total = range === null ? null : /\/(\d+)\s*$/.exec(range)?.[1];
    return total == null ? null : Number(total);
  }
  const length = header(response, 'content-length');
  if (length === null || !/^\d+$/.test(length.trim())) return null;
  const size = Number(length.trim());
  return size > 0 ? size : null;
}

/** True when the response is saved as a file rather than shown (`Content-Disposition: attachment`). */
export function isAttachment(response: ObservedResponse): boolean {
  const disposition = header(response, 'content-disposition');
  return disposition !== null && /^\s*attachment/i.test(disposition);
}

/** True when this response replaces the document in a tab (a new page), so the tab's list starts over. */
export function isNavigation(response: ObservedResponse): boolean {
  return (
    response.type === 'main_frame' &&
    response.tabId >= 0 &&
    response.statusCode >= 200 &&
    response.statusCode < 300 &&
    !isAttachment(response)
  );
}

/**
 * Decides whether a response is media worth offering, and what kind:
 *
 * - HLS and DASH manifests by content type, or by `.m3u8` / `.mpd` when the server
 *   sends a generic type (many send `text/plain` or `application/octet-stream`).
 * - Whole video and audio files (`video/*`, `audio/*`, or a media extension with a
 *   generic type) loaded by a media element, a frame or the tab itself.
 * - Never stream segments, error answers, redirects, downloads (attachments, which
 *   the capture flow handles), anything outside a tab, or addresses longer than
 *   `MAX_MEDIA_URL_LENGTH`.
 */
export function classifyResponse(response: ObservedResponse): Detection | null {
  if (response.tabId < 0) return null;
  if (response.documentLifecycle === 'prerender') return null;
  if (response.statusCode < 200 || response.statusCode >= 300) return null;
  if (!isWebUrl(response.url)) return null;
  if (response.url.length > MAX_MEDIA_URL_LENGTH) return null;
  if (isAttachment(response)) return null;

  const mime = mimeOf(response);
  const byUrl = classifyUrl(response.url);
  const ext = extensionOf(response.url);

  if ((mime !== null && HLS_TYPES.has(mime)) || (byUrl?.kind === 'hls' && isPlainText(mime))) {
    return { kind: 'hls', url: response.url, mime, size: null, audio: false };
  }
  if (
    (mime !== null && DASH_TYPES.has(mime)) ||
    (byUrl?.kind === 'dash' && (isPlainText(mime) || mime === 'application/xml' || mime === 'text/xml'))
  ) {
    return { kind: 'dash', url: response.url, mime, size: null, audio: false };
  }

  if (!FILE_REQUEST_TYPES.has(response.type)) return null;
  if (mime !== null && SEGMENT_TYPES.has(mime)) return null;
  if (ext !== null && SEGMENT_EXTENSIONS.has(ext)) return null;

  if (isMediaMime(mime) && mime !== null) {
    return {
      kind: 'direct',
      url: response.url,
      mime,
      size: sizeOf(response),
      audio: mime.startsWith('audio/'),
    };
  }
  if (byUrl?.kind === 'direct' && isGenericMime(mime)) {
    return { kind: 'direct', url: response.url, mime, size: sizeOf(response), audio: byUrl.audio };
  }
  return null;
}

function isMediaMime(mime: string | null): boolean {
  return mime !== null && (mime.startsWith('video/') || mime.startsWith('audio/'));
}

/**
 * Types a server sends when it does not know better, so the address extension may
 * decide. A page (`text/html`) or an API answer (`application/json`) at an address
 * ending in `.m3u8` is an error page or a wrapper, not a manifest.
 */
function isPlainText(mime: string | null): boolean {
  return isGenericMime(mime) || mime === 'text/plain';
}

function isGenericMime(mime: string | null): boolean {
  return mime === null || mime === 'application/octet-stream' || mime === 'binary/octet-stream';
}
