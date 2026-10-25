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
    path: /^\/(watch|shorts\/|live\/|embed\/|v\/|clip\/)/,
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
  {
    name: 'Bilibili',
    hosts: ['bilibili.com'],
    path: /^\/(video\/|bangumi\/play\/|audio\/au\d+)/,
  },
  { name: 'Rumble', hosts: ['rumble.com'], path: /^\/(v[a-z0-9]+-|embed\/)/ },
  { name: 'Streamable', hosts: ['streamable.com'] },
  { name: 'Odysee', hosts: ['odysee.com'], path: /^\/@[^/]+\/[^/]+/ },
  { name: 'Kick', hosts: ['kick.com'], path: /^\/[^/]+\/(videos|clips)\// },
  { name: 'Bluesky', hosts: ['bsky.app'], path: /^\/profile\/[^/]+\/post\// },
  { name: 'Bilibili', hosts: ['bilibili.tv'], path: /^\/([a-z]{2}\/)?video\/\d+/ },
  { name: 'Niconico', hosts: ['nicovideo.jp'], path: /^\/watch\// },
  { name: 'VK', hosts: ['vk.com', 'vkvideo.ru'], path: /^\/(video|clip)-?\d+_\d+/ },
  { name: 'OK', hosts: ['ok.ru'], path: /^\/video\/\d+/ },
  { name: 'Rutube', hosts: ['rutube.ru'], path: /^\/video\/[0-9a-f]{32}/ },
  { name: 'Youku', hosts: ['youku.com'], path: /^\/v_show\// },
  { name: 'Douyin', hosts: ['douyin.com'], path: /^\/video\/\d+/ },
  { name: 'Weibo', hosts: ['weibo.com'], path: /^\/tv\/show\// },
  { name: 'AcFun', hosts: ['acfun.cn'], path: /^\/v\/ac\d+/ },
  { name: 'Xigua', hosts: ['ixigua.com'], path: /^\/\d+/ },
  { name: 'Tencent Video', hosts: ['v.qq.com'], path: /^\/x\/(cover|page)\// },
  { name: 'iQIYI', hosts: ['iqiyi.com'], path: /^\/v_/ },
  { name: 'iQIYI', hosts: ['iq.com'], path: /^\/play\// },
  { name: 'Mango TV', hosts: ['mgtv.com'], path: /^\/b\/\d+\/\d+/ },
  { name: 'Huya', hosts: ['huya.com'], path: /^\/video\/play\// },
  { name: 'Douyu', hosts: ['v.douyu.com'], path: /^\/show\// },
  { name: 'Zhihu', hosts: ['zhihu.com'], path: /^\/zvideo\/\d+/ },
  { name: 'Naver TV', hosts: ['tv.naver.com'], path: /^\/v\/\d+/ },
  { name: 'Chzzk', hosts: ['chzzk.naver.com'], path: /^\/video\/\d+/ },
  { name: 'TVer', hosts: ['tver.jp'], path: /^\/episodes\// },
  {
    name: 'NHK World',
    hosts: ['nhk.or.jp'],
    path: /^\/nhkworld\/[a-z]+\/ondemand\/(video|audio)\//,
  },
  { name: 'Vidio', hosts: ['vidio.com'], path: /^\/watch\/\d+/ },
  { name: 'TED', hosts: ['ted.com'], path: /^\/talks\/[^/]+/ },
  { name: 'Loom', hosts: ['loom.com'], path: /^\/share\/[0-9a-f]+/ },
  { name: 'BitChute', hosts: ['bitchute.com'], path: /^\/video\/[^/]+/ },
  { name: '9GAG', hosts: ['9gag.com'], path: /^\/gag\// },
  { name: 'Medal', hosts: ['medal.tv'], path: /^\/games\/[^/]+\/clips\// },
  { name: 'IMDb', hosts: ['imdb.com'], path: /^\/video\/vi\d+/ },
  { name: 'Snapchat', hosts: ['snapchat.com'], path: /^\/spotlight\// },
  { name: 'Truth Social', hosts: ['truthsocial.com'], path: /^\/@[^/]+\/posts\/\d+/ },
  { name: 'Coub', hosts: ['coub.com'], path: /^\/view\// },
  { name: 'Newgrounds', hosts: ['newgrounds.com'], path: /^\/(portal\/view|audio\/listen)\/\d+/ },
  { name: 'Audiomack', hosts: ['audiomack.com'], path: /^\/([^/]+\/)?(song|album)\// },
  { name: 'Audius', hosts: ['audius.co'], path: /^\/[^/]+\/[^/]+/ },
  { name: 'TwitCasting', hosts: ['twitcasting.tv'], path: /^\/[^/]+\/movie\/\d+/ },
  { name: 'Kakao TV', hosts: ['tv.kakao.com'], path: /^\/channel\/\d+\/cliplink\/\d+/ },
  { name: 'SOOP', hosts: ['vod.sooplive.com'], path: /^\/player\/\d+/i },
  { name: 'Ximalaya', hosts: ['ximalaya.com'], path: /^\/(\d+\/)?sound\/\d+/ },
  { name: 'Sohu', hosts: ['tv.sohu.com'], path: /^\/v\// },
  { name: 'Viu', hosts: ['viu.com'], path: /^\/[a-z]{2}\/media\/\d+/ },
  { name: 'MX Player', hosts: ['mxplayer.in'], path: /^\/(show|movie)\/watch-/ },
  { name: 'Zing MP3', hosts: ['zingmp3.vn'], path: /^\/(bai-hat|video-clip)\// },
  { name: 'Aparat', hosts: ['aparat.com'], path: /^\/v\// },
  { name: 'CDA', hosts: ['cda.pl'], path: /^\/video\// },
  { name: 'ARD Mediathek', hosts: ['ardmediathek.de'], path: /^\/([^/]+\/)?video\// },
  { name: 'Arte', hosts: ['arte.tv'], path: /^\/[a-z]{2}\/videos\/\d/ },
  { name: 'RaiPlay', hosts: ['raiplay.it'], path: /^\/video\// },
  { name: 'NRK TV', hosts: ['tv.nrk.no'], path: /^\/(program|serie)\// },
  { name: 'Pornhub', hosts: ['pornhub.com'], path: /^\/(view_video\.php|embed\/)/ },
  { name: 'XVideos', hosts: ['xvideos.com'], path: /^\/(video[.\d]|embedframe\/)/ },
  { name: 'XNXX', hosts: ['xnxx.com'], path: /^\/video-/ },
  { name: 'xHamster', hosts: ['xhamster.com'], path: /^\/videos\// },
  { name: 'RedTube', hosts: ['redtube.com'], path: /^\/\d+/ },
  { name: 'YouPorn', hosts: ['youporn.com'], path: /^\/watch\/\d+/ },
  { name: 'SpankBang', hosts: ['spankbang.com'], path: /^\/[a-z0-9]+\/(video|embed)\// },
  { name: 'Eporner', hosts: ['eporner.com'], path: /^\/(video-|hd-porn\/)/ },
  { name: 'RedGifs', hosts: ['redgifs.com'], path: /^\/watch\// },
  { name: 'Beeg', hosts: ['beeg.com'], path: /^\/-?\d+/ },
  { name: 'Txxx', hosts: ['txxx.com'], path: /^\/videos\/\d+/ },
  { name: 'TNAFlix', hosts: ['tnaflix.com'], path: /^\/[^/]+\/[^/]+\/video\d+/ },
  { name: 'DrTuber', hosts: ['drtuber.com'], path: /^\/video\/\d+/ },
  { name: 'YouJizz', hosts: ['youjizz.com'], path: /^\/videos\// },
  { name: 'Iwara', hosts: ['iwara.tv'], path: /^\/video\// },
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
