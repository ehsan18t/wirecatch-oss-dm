// The video sites both apps take (D51): the listed sites, narrowed to pages that show one video, and the sites the person added.

import { describe, expect, it } from 'vitest';
import { videoSiteFor } from './video-sites';

describe('videoSiteFor', () => {
  it.each([
    ['https://www.youtube.com/watch?v=abc', 'YouTube'],
    ['https://m.youtube.com/shorts/abc', 'YouTube'],
    ['https://youtu.be/abc', 'YouTube'],
    ['https://vimeo.com/123456', 'Vimeo'],
    ['https://www.dailymotion.com/video/x8abc', 'Dailymotion'],
    ['https://www.twitch.tv/videos/123', 'Twitch'],
    ['https://clips.twitch.tv/SomeClip', 'Twitch'],
    ['https://x.com/someone/status/123', 'X'],
    ['https://twitter.com/someone/status/123', 'X'],
    ['https://www.facebook.com/watch/?v=1', 'Facebook'],
    ['https://www.instagram.com/reel/abc/', 'Instagram'],
    ['https://www.tiktok.com/@someone/video/123', 'TikTok'],
    ['https://www.reddit.com/r/videos/comments/abc/title/', 'Reddit'],
    ['https://soundcloud.com/artist/track', 'SoundCloud'],
    ['https://kick.com/someone/clips/clip_01ABC', 'Kick'],
    ['https://www.bilibili.tv/en/video/2019955076', 'Bilibili'],
    ['https://www.nicovideo.jp/watch/sm9', 'Niconico'],
    ['https://vk.com/video-12345_67890', 'VK'],
    ['https://vk.com/clip12345_67890', 'VK'],
    ['https://vkvideo.ru/video-12345_67890', 'VK'],
    ['https://ok.ru/video/12345678', 'OK'],
    ['https://rutube.ru/video/0123456789abcdef0123456789abcdef/', 'Rutube'],
    ['https://v.youku.com/v_show/id_XMTc1ODE5Njcy.html', 'Youku'],
    ['https://www.douyin.com/video/6961737553342991651', 'Douyin'],
    ['https://weibo.com/tv/show/1034:4797699866951785', 'Weibo'],
    ['https://www.youtube.com/clip/UgkxAbc', 'YouTube'],
    ['https://www.bilibili.com/bangumi/play/ep12345', 'Bilibili'],
    ['https://www.bilibili.com/audio/au1003142', 'Bilibili'],
    ['https://www.acfun.cn/v/ac35457073', 'AcFun'],
    ['https://www.ixigua.com/6996881461559165471', 'Xigua'],
    ['https://v.qq.com/x/cover/abcdefghijklmno/a0012abcdef.html', 'Tencent Video'],
    ['https://www.iqiyi.com/v_19rr7qhfg0.html', 'iQIYI'],
    ['https://www.iq.com/play/some-title-1e4tq5dd5o0', 'iQIYI'],
    ['https://www.mgtv.com/b/301817/3826653.html', 'Mango TV'],
    ['https://www.huya.com/video/play/1002412640.html', 'Huya'],
    ['https://v.douyu.com/show/a4Jj7llO9M2vk5Lw', 'Douyu'],
    ['https://www.zhihu.com/zvideo/1342930761977176064', 'Zhihu'],
    ['https://tv.naver.com/v/12345678', 'Naver TV'],
    ['https://chzzk.naver.com/video/12345', 'Chzzk'],
    ['https://tver.jp/episodes/ep83nf3w4p', 'TVer'],
    ['https://www3.nhk.or.jp/nhkworld/en/ondemand/video/2049126/', 'NHK World'],
    ['https://www.vidio.com/watch/7550-some-title', 'Vidio'],
    ['https://www.ted.com/talks/some_speaker_some_title', 'TED'],
    ['https://www.loom.com/share/0123456789abcdef0123456789abcdef', 'Loom'],
    ['https://www.bitchute.com/video/UGlrF9o9b-Q/', 'BitChute'],
    ['https://9gag.com/gag/ae5Ag7B', '9GAG'],
    ['https://medal.tv/games/valorant/clips/abc123', 'Medal'],
    ['https://www.imdb.com/video/vi1234567/', 'IMDb'],
    ['https://www.snapchat.com/spotlight/W7_EDlXWTBiXAEEniNoMPwAAYYWtidGhudGZp', 'Snapchat'],
    ['https://truthsocial.com/@someone/posts/108779000807761862', 'Truth Social'],
    ['https://coub.com/view/5u5n1', 'Coub'],
    ['https://www.newgrounds.com/portal/view/1', 'Newgrounds'],
    ['https://www.newgrounds.com/audio/listen/549479', 'Newgrounds'],
    ['https://audiomack.com/song/roosh-williams/extraordinary', 'Audiomack'],
    ['https://audius.co/voltra/radar-103692', 'Audius'],
    ['https://twitcasting.tv/ivetesangalo/movie/2357609', 'TwitCasting'],
    ['https://tv.kakao.com/channel/2671005/cliplink/301965083', 'Kakao TV'],
    ['https://vod.sooplive.com/player/192805325', 'SOOP'],
    ['https://www.ximalaya.com/sound/47740352/', 'Ximalaya'],
    ['https://tv.sohu.com/v/MjAyMzA2MTQvbjYwMTMxNTE5Mi5zaHRtbA==.html', 'Sohu'],
    ['https://www.viu.com/en/media/1130599965', 'Viu'],
    [
      'https://www.mxplayer.in/show/watch-miya-biwi-aur-murder/season-1/episode-1-online-ca17972f052449de6633129ddd7db90b',
      'MX Player',
    ],
    ['https://zingmp3.vn/video-clip/Suong-Hoa-Dua-Loi-K-ICM-RYO/ZO8ZF7C7.html', 'Zing MP3'],
    ['https://www.aparat.com/v/wP8On', 'Aparat'],
    ['https://www.cda.pl/video/5749950c', 'CDA'],
    [
      'https://www.ardmediathek.de/video/some-show/some-title/mdr/Y3JpZDovL21kci5kZS9',
      'ARD Mediathek',
    ],
    ['https://www.arte.tv/fr/videos/109067-000-A/la-loi-de-teheran/', 'Arte'],
    [
      'https://www.raiplay.it/video/2021/11/Blanca-S1E1-Senza-occhi-b1255a4a-8e72-4a2f-b9f3-fc1308e00736.html',
      'RaiPlay',
    ],
    ['https://tv.nrk.no/program/MDDP12000117', 'NRK TV'],
    ['https://www.pornhub.com/view_video.php?viewkey=abc', 'Pornhub'],
    ['https://www.pornhub.com/embed/abc', 'Pornhub'],
    ['https://www.xvideos.com/video12345678/some_title', 'XVideos'],
    ['https://www.xvideos.com/video.ohmbbdh1234/some_title', 'XVideos'],
    ['https://www.xnxx.com/video-abc12de/some_title', 'XNXX'],
    ['https://xhamster.com/videos/some-title-1234567', 'xHamster'],
    ['https://www.redtube.com/12345678', 'RedTube'],
    ['https://www.youporn.com/watch/12345678/some-title/', 'YouPorn'],
    ['https://spankbang.com/3vvn9/video/some+title', 'SpankBang'],
    ['https://www.eporner.com/hd-porn/95008/some-title/', 'Eporner'],
    ['https://www.redgifs.com/watch/squeakyhelplesswisent', 'RedGifs'],
    ['https://beeg.com/-0983946056129650', 'Beeg'],
    ['https://txxx.com/videos/16574965/some-title/', 'Txxx'],
    ['https://www.tnaflix.com/some-category/Some-title/video553878', 'TNAFlix'],
    ['https://www.drtuber.com/video/1740434/some-title', 'DrTuber'],
    ['https://www.youjizz.com/videos/some-title-2189178.html', 'YouJizz'],
    ['https://www.iwara.tv/video/k2ayoueezfkx6gvq', 'Iwara'],
  ])('takes %s as a video page', (url, name) => {
    expect(videoSiteFor(url)).toBe(name);
  });

  it.each([
    'https://www.youtube.com/',
    'https://www.youtube.com/results?search_query=x',
    'https://vimeo.com/',
    'https://x.com/home',
    'https://www.pornhub.com/video/search?search=x',
    'https://vk.com/feed',
    'https://www.zhihu.com/question/123',
    'https://www.mgtv.com/tv/',
    'https://www.ted.com/talks',
    'https://www.xvideos.com/tags/x',
    'https://www.redtube.com/top',
    'https://notyoutube.com/watch?v=abc',
    'https://example.com/watch?v=abc',
    'file:///C:/video.mp4',
    'not a url',
    null,
  ])('does not take %s', (url) => {
    expect(videoSiteFor(url)).toBeNull();
  });

  it('takes any page but the home page of a site the person added, as that site', () => {
    expect(videoSiteFor('https://videos.example.org/talks/42', ['example.org'])).toBe(
      'example.org',
    );
    expect(videoSiteFor('https://example.org/', ['example.org'])).toBeNull();
    expect(videoSiteFor('https://badexample.org/talks/42', ['example.org'])).toBeNull();
    // The listed sites come first, and keep their own names.
    expect(videoSiteFor('https://youtu.be/abc', ['youtu.be'])).toBe('YouTube');
  });
});
