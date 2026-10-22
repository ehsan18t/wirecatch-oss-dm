// The sites whose pages show videos (D51), in one list for both apps: the extension offers to send such a page to the app, and the app reads a link to one for its video when it is added. The person can add sites of their own (`ExtensionSettings::video_sites`), which both apps take with this list.

import { normalizeSite } from './extension-settings';

/**
 * Well-known video and audio sites whose pages need an extractor (the app's media add-on) to find
 * the media, because the player streams it in pieces. Being listed is not a promise that a site
 * works: the app decides that, and explains it (or offers to install the media add-on) when it
 * reads the page. Each entry covers the host and its subdomains; `path` narrows it to pages that
 * show one video or track, so a home page or a search page is not taken.
 */
interface VideoSite {
  name: string;
  hosts: readonly string[];
  /** Matched against the path; absent means any path except the site's home page. */
  path?: RegExp;
}

export const VIDEO_SITES: readonly VideoSite[] = [
  {
    name: 'YouTube',
    hosts: ['youtube.com', 'youtube-nocookie.com'],
    path: /^\/(watch|shorts\/|live\/|embed\/|v\/)/,
  },
  { name: 'YouTube', hosts: ['youtu.be'] },
  {
    name: 'Vimeo',
    hosts: ['vimeo.com'],
    path: /^\/(\d+|video\/\d+|channels\/[^/]+\/\d+|[^/]+\/[^/]+\/video\/\d+)/,
  },
  { name: 'Dailymotion', hosts: ['dailymotion.com'], path: /^\/(video|embed\/video)\// },
  { name: 'Dailymotion', hosts: ['dai.ly'] },
  { name: 'Twitch', hosts: ['twitch.tv'], path: /^\/(videos\/\d+|[^/]+\/(clip|v|video)\/)/ },
  { name: 'Twitch', hosts: ['clips.twitch.tv'] },
  { name: 'X', hosts: ['x.com', 'twitter.com'], path: /^\/[^/]+\/status\/\d+/ },
  {
    name: 'Facebook',
    hosts: ['facebook.com'],
    path: /^\/(watch|reel\/|[^/]+\/videos\/|share\/[rv]\/)/,
  },
  { name: 'Facebook', hosts: ['fb.watch'] },
  { name: 'Instagram', hosts: ['instagram.com'], path: /^\/([^/]+\/)?(p|reel|reels|tv)\/[^/]+/ },
  { name: 'TikTok', hosts: ['tiktok.com'], path: /^\/@[^/]+\/video\/\d+/ },
  { name: 'TikTok', hosts: ['vm.tiktok.com', 'vt.tiktok.com'] },
  { name: 'Reddit', hosts: ['reddit.com'], path: /^\/r\/[^/]+\/comments\// },
  { name: 'SoundCloud', hosts: ['soundcloud.com'], path: /^\/[^/]+\/[^/]+/ },
  { name: 'Bandcamp', hosts: ['bandcamp.com'], path: /^\/(track|album)\// },
  { name: 'Mixcloud', hosts: ['mixcloud.com'], path: /^\/[^/]+\/[^/]+/ },
  { name: 'Bilibili', hosts: ['bilibili.com'], path: /^\/video\// },
  { name: 'Rumble', hosts: ['rumble.com'], path: /^\/(v[a-z0-9]+-|embed\/)/ },
  { name: 'Streamable', hosts: ['streamable.com'] },
  { name: 'Odysee', hosts: ['odysee.com'], path: /^\/@[^/]+\/[^/]+/ },
  { name: 'Kick', hosts: ['kick.com'], path: /^\/[^/]+\/videos\// },
  { name: 'Bluesky', hosts: ['bsky.app'], path: /^\/profile\/[^/]+\/post\// },
];

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
