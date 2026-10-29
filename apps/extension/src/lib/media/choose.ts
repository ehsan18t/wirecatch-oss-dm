import { videoSiteFor, type MediaKind } from '@wirecatch/protocol';
import { fileNameFromUrl } from '../file-types';
import { parseUrl } from '../url';
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
  /** The document that chose its address (`MediaItem.requestedBy`); the page itself for `page`. */
  requestedBy: string | null;
  /** The `Cookie` header its request went out with (`MediaItem.sentCookies`); missing for `page`. */
  sentCookies?: string | null;
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
  /** The frame the element is in (0 for the tab's page); its own media comes first. */
  frameId?: number | null;
  /** Sites with video pages the person added to the ones the apps know (D51). */
  videoSites?: readonly string[];
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
    requestedBy: item.requestedBy,
    ...(item.sentCookies === undefined ? {} : { sentCookies: item.sentCookies }),
  };
}

/** The page itself, when it is on a video site, listed or added by the person, that the media add-on may be able to read (the frame first, for embedded players). */
export function pageTarget(
  frameUrl: string | null,
  tabUrl: string | null,
  videoSites: readonly string[] = [],
): MediaTarget | null {
  for (const url of [frameUrl, tabUrl]) {
    const site = videoSiteFor(url, videoSites);
    if (site !== null && url !== null) {
      return {
        key: `page:${url}`,
        kind: 'page',
        url,
        mime: null,
        size: null,
        audio: false,
        site,
        requestedBy: url,
      };
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
 * 2. Otherwise a stream manifest detected in the tab (HLS or DASH), the newest one:
 *    on a page that swaps videos it is the one playing now. Variant playlists of a
 *    stream are not listed, so this is normally a master playlist.
 * 3. Otherwise a direct file detected in the tab.
 *
 * In steps 2 and 3, media loaded by the element's own frame wins over the rest of the tab.
 * 4. Otherwise the page, on a well-known video site (the app's media add-on reads it).
 *
 * Null means nothing downloadable is known, so the button is not shown.
 */
export function chooseTarget(
  items: readonly MediaItem[],
  context: ButtonContext,
): MediaTarget | null {
  const src = context.elementSrc;
  if (src !== null) {
    const loaded = items.find((i) => i.url === src);
    if (loaded !== undefined) return fromItem(loaded);
  }
  // Media the element's own frame loaded comes first: on a page with an embedded
  // player, that is the player the person clicked on.
  const frame = context.frameId ?? null;
  const own = frame === null ? [] : items.filter((i) => i.frameId === frame);
  for (const list of [own, items]) {
    const stream = list.findLast((i) => i.kind === 'hls' || i.kind === 'dash');
    if (stream !== undefined) return fromItem(stream);
  }
  for (const list of [own, items]) {
    const file = list.find((i) => i.kind === 'direct');
    if (file !== undefined) return fromItem(file);
  }
  return pageTarget(context.frameUrl, context.tabUrl, context.videoSites);
}

/** Everything the popup can offer for a tab: detected media, then the page on a video site. */
export function popupTargets(
  items: readonly MediaItem[],
  tabUrl: string | null,
  videoSites: readonly string[] = [],
): MediaTarget[] {
  const targets = items.map(fromItem);
  const page = pageTarget(null, tabUrl, videoSites);
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
        detail: 'Wirecatch gets the video with its media add-on.',
      };
  }
}

function hostOf(url: string): string {
  return parseUrl(url)?.hostname ?? url;
}
