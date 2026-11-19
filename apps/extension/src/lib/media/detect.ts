/**
 * Rules for spotting media in network responses the browser reports through
 * `webRequest`. Observing only: nothing here can change, delay or block a request.
 */

import { DEFAULT_MEDIA_TABLES, type MediaTables } from '@wirecatch/protocol';
import { MAX_MEDIA_URL_LENGTH } from './messages';
import { isWebUrl, parseUrl } from '../url';

/** How detected media is delivered (the page kind is never detected from traffic). */
export type DetectedKind = 'hls' | 'dash' | 'direct';

/** One request as `webRequest.onSendHeaders` reports it, in both Chromium and Firefox. */
export interface ObservedRequest {
  /** The same for the request and its response, and across its redirects. */
  requestId: string;
  tabId: number;
  /** In Chromium the `Cookie` header is among them only when asked for with `extraHeaders`. */
  requestHeaders?: { name: string; value?: string }[];
  /** Firefox only: true for requests from private windows. Chromium leaves it out. */
  incognito?: boolean;
}

/** The `Cookie` header a request was sent with, or null when it was sent without one. */
export function sentCookies(request: ObservedRequest): string | null {
  const cookie = request.requestHeaders?.find((h) => h.name.toLowerCase() === 'cookie');
  return cookie?.value ?? null;
}

/** One response as `webRequest.onResponseStarted` reports it, in both Chromium and Firefox. */
export interface ObservedResponse {
  /** The request's id (`ObservedRequest.requestId`). */
  requestId: string;
  url: string;
  /** -1 when the request does not belong to a tab (service workers, the browser itself, other extensions). */
  tabId: number;
  /** Resource type: `main_frame`, `media`, `xmlhttprequest` and so on. */
  type: string;
  statusCode: number;
  responseHeaders?: { name: string; value?: string }[];
  /** Firefox only: true for requests from private windows. Chromium leaves it out. */
  incognito?: boolean;
  /** Chromium only: `prerender` for pages loading in the background before anyone opens them. */
  documentLifecycle?: string;
  /** The frame that made the request: 0 for the tab's page, another number for a frame inside it. */
  frameId?: number;
  /** Firefox only: the address of the document the response is for (the frame's, or the tab page's). */
  documentUrl?: string;
  /** Chromium only: the origin of the document that made the request. */
  initiator?: string;
}

/** The document that requested a response, as far as the browser says (see `MediaItem.requestedBy`). */
export function requestedBy(response: ObservedResponse): string | null {
  return response.documentUrl ?? response.initiator ?? null;
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

/** The app's media tables (D279) as sets, for lookups. */
export interface MediaRules {
  hlsExtensions: ReadonlySet<string>;
  hlsTypes: ReadonlySet<string>;
  dashExtensions: ReadonlySet<string>;
  dashTypes: ReadonlySet<string>;
  videoExtensions: ReadonlySet<string>;
  audioExtensions: ReadonlySet<string>;
  /** Pieces of a stream rather than whole files: listing them would only add noise. */
  segmentExtensions: ReadonlySet<string>;
  segmentTypes: ReadonlySet<string>;
  /** Pages play short interface sounds as media; real recordings are far larger. Files of unknown size are kept. */
  minDirectBytes: number;
}

const built = new WeakMap<MediaTables, MediaRules>();

/** The rules `tables` say, built once per copy of the tables. */
export function mediaRules(tables: MediaTables): MediaRules {
  let rules = built.get(tables);
  if (rules === undefined) {
    rules = {
      hlsExtensions: new Set(tables.hls_extensions),
      hlsTypes: new Set(tables.hls_types),
      dashExtensions: new Set(tables.dash_extensions),
      dashTypes: new Set(tables.dash_types),
      videoExtensions: new Set(tables.video_extensions),
      audioExtensions: new Set(tables.audio_extensions),
      segmentExtensions: new Set(tables.segment_extensions),
      segmentTypes: new Set(tables.segment_types),
      minDirectBytes: tables.min_direct_bytes,
    };
    built.set(tables, rules);
  }
  return rules;
}

/** The rules the extension ships with, for the time before the app first answers. */
export const DEFAULT_MEDIA_RULES = mediaRules(DEFAULT_MEDIA_TABLES);

/** Generic XML types a DASH manifest may be served as. */
const XML_TYPES = new Set(['application/xml', 'text/xml']);

/**
 * Resource types whose video or audio responses are whole files. Media fetched by
 * scripts (`xmlhttprequest`, which includes `fetch`) is almost always a slice fed to
 * a player through Media Source Extensions, so it is not listed as a file.
 */
const FILE_REQUEST_TYPES = new Set(['media', 'main_frame', 'object']);

/** HTTP 206: the answer holds part of the file, and `Content-Range` says its total size. */
const HTTP_PARTIAL_CONTENT = 206;

/** The 2xx status codes: the request succeeded. */
const HTTP_SUCCESS = { from: 200, below: 300 };

function isSuccess(status: number): boolean {
  return status >= HTTP_SUCCESS.from && status < HTTP_SUCCESS.below;
}

/** Longest media type kept; RFC 6838 allows 127 characters for each half. */
const MAX_MIME_LENGTH = 255;

/** The lower-case extension of the last path segment, or null. */
export function extensionOf(url: string): string | null {
  const path = parseUrl(url)?.pathname;
  if (path === undefined) return null;
  const last = path.slice(path.lastIndexOf('/') + 1);
  const dot = last.lastIndexOf('.');
  if (dot <= 0 || dot === last.length - 1) return null;
  return last.slice(dot + 1).toLowerCase();
}

/** What an address alone says about the media it points to (by extension), or null. */
export function classifyUrl(
  url: string,
  rules: MediaRules = DEFAULT_MEDIA_RULES,
): { kind: DetectedKind; audio: boolean } | null {
  if (!isWebUrl(url)) return null;
  const ext = extensionOf(url);
  if (ext === null) return null;
  if (rules.hlsExtensions.has(ext)) return { kind: 'hls', audio: false };
  if (rules.dashExtensions.has(ext)) return { kind: 'dash', audio: false };
  if (rules.videoExtensions.has(ext)) return { kind: 'direct', audio: false };
  if (rules.audioExtensions.has(ext)) return { kind: 'direct', audio: true };
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
  if (response.statusCode === HTTP_PARTIAL_CONTENT) {
    const range = header(response, 'content-range');
    const total = range === null ? undefined : /\/(\d+)\s*$/.exec(range)?.[1];
    return total === undefined ? null : Number(total);
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
    isSuccess(response.statusCode) &&
    !isAttachment(response)
  );
}

/**
 * Decides whether a response is media worth offering, and what kind:
 *
 * - HLS and DASH manifests by content type, or by their extension (`.m3u8`, `.mpd`) when the
 *   server sends a generic type (many send `text/plain` or `application/octet-stream`).
 * - Whole video and audio files (`video/*`, `audio/*`, or a media extension with a
 *   generic type) loaded by a media element, a frame or the tab itself.
 * - Never stream segments, error answers, redirects, downloads (attachments, which
 *   the capture flow handles), anything outside a tab, or addresses longer than
 *   `MAX_MEDIA_URL_LENGTH`, nor direct files smaller than `rules.minDirectBytes`.
 *
 * The tables are the app's (D279); the order of the checks stays here.
 */
export function classifyResponse(
  response: ObservedResponse,
  rules: MediaRules = DEFAULT_MEDIA_RULES,
): Detection | null {
  if (rejectionOf(response) !== null) return null;
  const mime = mimeOf(response);
  return manifestOf(response.url, mime, rules) ?? directFileOf(response, mime, rules);
}

/** Why a response is never listed, whatever it contains. */
export type Rejection =
  'outside_tab' | 'prerender' | 'not_success' | 'not_web' | 'url_too_long' | 'attachment';

/** The checks every listed response passes, in order; the first that rejects it decides. */
const REJECTIONS: readonly {
  reason: Rejection;
  rejects: (response: ObservedResponse) => boolean;
}[] = [
  { reason: 'outside_tab', rejects: (r) => r.tabId < 0 },
  { reason: 'prerender', rejects: (r) => r.documentLifecycle === 'prerender' },
  { reason: 'not_success', rejects: (r) => !isSuccess(r.statusCode) },
  { reason: 'not_web', rejects: (r) => !isWebUrl(r.url) },
  { reason: 'url_too_long', rejects: (r) => r.url.length > MAX_MEDIA_URL_LENGTH },
  { reason: 'attachment', rejects: isAttachment },
];

/** Why a response cannot be media worth listing, or null when its content decides. */
export function rejectionOf(response: ObservedResponse): Rejection | null {
  return REJECTIONS.find(({ rejects }) => rejects(response))?.reason ?? null;
}

/** An HLS or DASH manifest, by its type, or by its extension when the type is generic. */
function manifestOf(url: string, mime: string | null, rules: MediaRules): Detection | null {
  const kind = manifestKind(url, mime, rules);
  return kind === null ? null : { kind, url, mime, size: null, audio: false };
}

function manifestKind(url: string, mime: string | null, rules: MediaRules): 'hls' | 'dash' | null {
  const byUrl = classifyUrl(url, rules)?.kind;
  if ((mime !== null && rules.hlsTypes.has(mime)) || (byUrl === 'hls' && isPlainText(mime))) {
    return 'hls';
  }
  const xmlOrPlain = isPlainText(mime) || (mime !== null && XML_TYPES.has(mime));
  if ((mime !== null && rules.dashTypes.has(mime)) || (byUrl === 'dash' && xmlOrPlain)) {
    return 'dash';
  }
  return null;
}

/** A whole video or audio file, loaded by a media element, a frame or the tab itself; never a stream segment. */
function directFileOf(
  response: ObservedResponse,
  mime: string | null,
  rules: MediaRules,
): Detection | null {
  if (!FILE_REQUEST_TYPES.has(response.type) || isSegment(response.url, mime, rules)) return null;
  const audio = directAudio(response.url, mime, rules);
  if (audio === null) return null;
  const size = sizeOf(response);
  if (size !== null && size < rules.minDirectBytes) return null;
  return { kind: 'direct', url: response.url, mime, size, audio };
}

function isSegment(url: string, mime: string | null, rules: MediaRules): boolean {
  const ext = extensionOf(url);
  return (
    (mime !== null && rules.segmentTypes.has(mime)) ||
    (ext !== null && rules.segmentExtensions.has(ext))
  );
}

/**
 * Whether a media file is audio: by its type, or by its extension when the type is
 * generic. Null when it is not a video or audio file.
 */
function directAudio(url: string, mime: string | null, rules: MediaRules): boolean | null {
  if (isMediaMime(mime)) return mime.startsWith('audio/');
  const byUrl = classifyUrl(url, rules);
  return byUrl?.kind === 'direct' && isGenericMime(mime) ? byUrl.audio : null;
}

function isMediaMime(mime: string | null): mime is string {
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
