// The sites whose pages show videos (D51), in one list for both apps: the extension offers to send such a page to the app, and the app reads a link to one for its video when it is added. The person can add sites of their own (`ExtensionSettings::video_sites`), which both apps take with this list.

import { normalizeSite } from './extension-settings';
import SITES from './video-sites.json' with { type: 'json' };

/**
 * Well-known video and audio sites whose pages need an extractor (the app's media add-on) to find
 * the media, because the player streams it in pieces. Being listed is not a promise that a site
 * works: the app decides that, and explains it (or offers to install the media add-on) when it
 * reads the page. Each entry covers the host and its subdomains; `path` narrows it to pages that
 * show one video or track, so a home page or a search page is not taken. The list is
 * `video-sites.json`, which the app's Rust reads too (`wirecatch_sites::video_site_for`).
 */
interface VideoSite {
  name: string;
  hosts: readonly string[];
  /** Matched against the path; absent means any path except the site's home page. */
  path?: RegExp;
}

export const VIDEO_SITES: readonly VideoSite[] = SITES.map((site) => ({
  name: site.name,
  hosts: site.hosts,
  ...(site.path === undefined
    ? {}
    : { path: new RegExp(site.path, 'ignore_case' in site && site.ignore_case ? 'i' : '') }),
}));

/** Whether `host` is `site` or one of its subdomains. */
function onSite(host: string, site: string): boolean {
  return host === site || host.endsWith(`.${site}`);
}

/**
 * The site's name when `url` is a page that shows a video: on a listed site, or on one of the
 * `added` sites (any page but the home page), named then by the site as added. Null otherwise.
 */
export function videoSiteFor(url: string | null, added: readonly string[] = []): string | null {
  const page = webPage(url);
  if (page === null) return null;
  const host = page.hostname.toLowerCase();
  const path = page.pathname;
  const listed = VIDEO_SITES.find(
    (site) =>
      site.hosts.some((h) => onSite(host, h)) &&
      (site.path === undefined ? path.length > 1 : site.path.test(path)),
  );
  if (listed !== undefined) return listed.name;
  if (path.length <= 1) return null;
  return added.map(normalizeSite).find((site) => site !== null && onSite(host, site)) ?? null;
}

/** `url` read as an http or https address, or null when it is not one. */
function webPage(url: string | null): URL | null {
  try {
    const parsed = new URL(url ?? '');
    return parsed.protocol === 'https:' || parsed.protocol === 'http:' ? parsed : null;
  } catch {
    // Not an absolute address, so not a page.
    return null;
  }
}
