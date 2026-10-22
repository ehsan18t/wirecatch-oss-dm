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
  ])('takes %s as a video page', (url, name) => {
    expect(videoSiteFor(url)).toBe(name);
  });

  it.each([
    'https://www.youtube.com/',
    'https://www.youtube.com/results?search_query=x',
    'https://vimeo.com/',
    'https://x.com/home',
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
