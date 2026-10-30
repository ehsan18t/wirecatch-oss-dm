// The video sites both apps take (D51): the listed sites, narrowed to pages that show one video, and the sites the person added. The cases are shared with the Rust reader of the list.

import { describe, expect, it } from 'vitest';
import CASES from './video-sites.cases.json';
import { videoSiteFor } from './video-sites';

describe('videoSiteFor', () => {
  it.each(CASES.pages.map(({ url, name }) => [url, name]))(
    'takes %s as a video page',
    (url, name) => {
      expect(videoSiteFor(url)).toBe(name);
    },
  );

  it.each([...CASES.not_pages, null])('does not take %s', (url) => {
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
