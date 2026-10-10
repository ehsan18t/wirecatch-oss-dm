import type { MediaKind } from '@wirecatch/protocol';
import { fileNameFromUrl } from '../file-types';
import { pageSiteFor } from './page-sites';
import type { MediaItem } from './registry';

/** One thing that can be sent to the app: detected media, or the page itself. */
export interface MediaTarget {
  /** Identifies it within the tab (the registry key, or `page:` for the page). */
  key: string;
  kind: MediaKind;
  url: string;
  mime: string | null;
  size: number | null;
  audio: boolean;
  /** For `page`: the site's name, for example YouTube. */
  site: string | null;
}

/** Where the button was clicked, as far as the content script can tell. */
export interface ButtonContext {
  /** The media element's `currentSrc`, when it has one (it may be `blob:` for players that stream in pieces). */
  elementSrc: string | null;
  /** True for an `<audio>` element. */
  elementIsAudio: boolean;
  /** Address of the frame the element is in (the tab's page, or an embedded player). */
  frameUrl: string | null;
  /** Address of the tab's page. */
  tabUrl: string | null;
}

function fromItem(item: MediaItem): MediaTarget {
  return {
    key: item.key,
    kind: item.kind,
    url: item.url,
    mime: item.mime,
    size: item.size,
    audio: item.audio,
    site: null,
  };
}

/** The page itself, when it is on a site the media add-on may be able to read (the frame first, for embedded players). */
export function pageTarget(frameUrl: string | null, tabUrl: string | null): MediaTarget | null {
  for (const url of [frameUrl, tabUrl]) {
    const site = pageSiteFor(url);
    if (site !== null && url !== null) {
      return { key: `page:${url}`, kind: 'page', url, mime: null, size: null, audio: false, site };
    }
  }
  return null;
}

/**
 * Picks what the media button sends:
 *
 * 1. The element's own file, but only when the browser itself loaded exactly that
 *    address as media in this tab (it is in the detected list). A page decides what
 *    its `<video src>` says, so an address the browser never loaded is not trusted:
 *    otherwise a page could make the button send any address on its site, with the
 *    person's cookies for it.
 * 2. Otherwise a stream manifest detected in the tab (HLS or DASH), the first one,
 *    which is normally the master playlist.
 * 3. Otherwise a direct file detected in the tab.
 * 4. Otherwise the page, on a well-known video site (the app's media add-on reads it).
 *
 * Null means nothing downloadable is known, so the button is not shown.
 */
export function chooseTarget(items: readonly MediaItem[], context: ButtonContext): MediaTarget | null {
  const src = context.elementSrc;
  if (src !== null) {
    const loaded = items.find((i) => i.url === src);
    if (loaded !== undefined) return fromItem(loaded);
  }
  const stream = items.find((i) => i.kind === 'hls' || i.kind === 'dash');
  if (stream !== undefined) return fromItem(stream);
  const file = items.find((i) => i.kind === 'direct');
  if (file !== undefined) return fromItem(file);
  return pageTarget(context.frameUrl, context.tabUrl);
}

/** Everything the popup can offer for a tab: detected media, then the page on a well-known video site. */
export function popupTargets(items: readonly MediaItem[], tabUrl: string | null): MediaTarget[] {
  const targets = items.map(fromItem);
  const page = pageTarget(null, tabUrl);
  if (page !== null) targets.push(page);
  return targets;
}

/** How the popup names one target: what it is, and where it comes from. */
export function describeTarget(target: MediaTarget): { title: string; detail: string } {
  const host = hostOf(target.url);
  const file = fileNameFromUrl(target.url);
  switch (target.kind) {
    case 'hls':
      return { title: 'Video stream (HLS)', detail: host };
    case 'dash':
      return { title: 'Video stream (DASH)', detail: host };
    case 'direct':
      return { title: target.audio ? 'Audio file' : 'Video file', detail: file ?? host };
    case 'page':
      return {
        title: `This ${target.site ?? 'page'} page`,
        detail: 'Wirecatch reads it with its media add-on.',
      };
  }
}

function hostOf(url: string): string {
  try {
    return new URL(url).hostname;
  } catch {
    return url;
  }
}
