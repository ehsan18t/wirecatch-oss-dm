/**
 * Well-known video and audio sites whose pages need an extractor (the media add-on
 * in the Wirecatch app) to find the media, because the player streams it in pieces
 * the extension cannot hand over on its own.
 *
 * This list only decides where the extension offers to send the page itself. It is
 * not a promise that the site works: the app decides what is actually supported,
 * and explains it (or offers to install the media add-on) when it gets the page.
 * Each entry covers the host and its subdomains; `path` narrows it to pages that
 * show one video or track, so a home page or a search page is not offered.
 */
interface PageSite {
  name: string;
  hosts: readonly string[];
  /** Matched against the path; absent means any path except the site's home page. */
  path?: RegExp;
}

export const PAGE_SITES: readonly PageSite[] = [
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

/** The site's name when `url` is a page the media add-on may be able to read, otherwise null. */
export function pageSiteFor(url: string | null | undefined): string | null {
  if (url == null) return null;
  let parsed: URL;
  try {
    parsed = new URL(url);
  } catch {
    return null;
  }
  if (parsed.protocol !== 'https:' && parsed.protocol !== 'http:') return null;
  const host = parsed.hostname.toLowerCase();
  const path = parsed.pathname;
  for (const site of PAGE_SITES) {
    if (!site.hosts.some((h) => host === h || host.endsWith(`.${h}`))) continue;
    if (site.path === undefined ? path.length > 1 : site.path.test(path)) return site.name;
  }
  return null;
}
