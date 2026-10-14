import { describe, expect, it } from 'vitest';
import {
  cookieQueries,
  cookiesToSend,
  isCrossSite,
  registrableDomain,
  siteOf,
  type CookieContext,
  type CookieLike,
} from '../src/lib/cookies';

const JAR: CookieLike[] = [
  { name: 'none', value: '1', sameSite: 'no_restriction' },
  { name: 'lax', value: '2', sameSite: 'lax' },
  { name: 'strict', value: '3', sameSite: 'strict' },
  { name: 'unspecified', value: '4', sameSite: 'unspecified' },
];

function names(cookies: CookieLike[]): string[] {
  return cookies.map((c) => c.name);
}

describe('sites', () => {
  it.each([
    ['www.example.com', 'example.com'],
    ['a.b.example.com', 'example.com'],
    ['example.com', 'example.com'],
    ['shop.example.co.uk', 'example.co.uk'],
    ['news.example.com.au', 'example.com.au'],
    ['alice.github.io', 'alice.github.io'],
    ['cdn.alice.github.io', 'alice.github.io'],
    ['bucket.s3.amazonaws.com', 'bucket.s3.amazonaws.com'],
    ['localhost', 'localhost'],
    ['192.168.1.10', '192.168.1.10'],
    ['[::1]', '[::1]'],
    ['WWW.Example.COM.', 'example.com'],
  ])('the registrable domain of %s is %s', (host, domain) => {
    expect(registrableDomain(host)).toBe(domain);
  });

  it('keeps the scheme in a site, and has none for other addresses', () => {
    expect(siteOf('https://www.example.com/a')).toBe('https://example.com');
    expect(siteOf('http://www.example.com/a')).toBe('http://example.com');
    expect(siteOf('blob:https://example.com/1')).toBeNull();
    expect(siteOf('not a url')).toBeNull();
    expect(siteOf(null)).toBeNull();
  });

  it('tells same-site from cross-site requests', () => {
    const url = 'https://cdn.example.com/file.zip';
    expect(isCrossSite(url, { initiator: 'https://www.example.com/page', navigation: true })).toBe(
      false,
    );
    expect(isCrossSite(url, { initiator: 'https://evil.test/page', navigation: true })).toBe(true);
    // Scheme counts, as in Chromium.
    expect(isCrossSite(url, { initiator: 'http://www.example.com/page', navigation: true })).toBe(
      true,
    );
    // Neighbours on shared hosting are different sites.
    expect(
      isCrossSite('https://alice.github.io/f.zip', {
        initiator: 'https://bob.github.io/',
        navigation: true,
      }),
    ).toBe(true);
    // Unknown starting page: treated as cross-site.
    expect(isCrossSite(url, { initiator: null, navigation: true })).toBe(true);
    // Redirected from another site.
    expect(
      isCrossSite(url, {
        initiator: 'https://www.example.com/page',
        originalUrl: 'https://tracker.test/r',
        navigation: true,
      }),
    ).toBe(true);
    // A same-site frame inside a page on another site.
    expect(
      isCrossSite(url, {
        initiator: 'https://www.example.com/embed',
        topLevel: 'https://news.test/',
        navigation: false,
      }),
    ).toBe(true);
  });
});

describe('cookiesToSend', () => {
  const url = 'https://files.example.com/report.pdf';

  it('sends every cookie for a same-site request', () => {
    const context: CookieContext = { initiator: 'https://example.com/account', navigation: true };
    expect(names(cookiesToSend(JAR, url, context))).toEqual([
      'none',
      'lax',
      'strict',
      'unspecified',
    ]);
  });

  it('never sends SameSite=Strict cookies across sites, but sends Lax for a top-level navigation', () => {
    const context: CookieContext = { initiator: 'https://other.test/links', navigation: true };
    expect(names(cookiesToSend(JAR, url, context))).toEqual(['none', 'lax', 'unspecified']);
  });

  it('sends only SameSite=None cookies with media another site loads', () => {
    const context: CookieContext = {
      initiator: 'https://other.test/watch',
      topLevel: 'https://other.test/watch',
      navigation: false,
    };
    expect(names(cookiesToSend(JAR, url, context))).toEqual(['none']);
  });

  it('withholds Strict cookies when the starting page is unknown', () => {
    expect(names(cookiesToSend(JAR, url, { initiator: null, navigation: true }))).toEqual([
      'none',
      'lax',
      'unspecified',
    ]);
  });

  it('keeps cookies whose SameSite state the browser does not report', () => {
    const cookies = [{ name: 'old', value: '1' }];
    expect(
      cookiesToSend(cookies, url, { initiator: 'https://other.test/', navigation: false }),
    ).toEqual(cookies);
  });
});

describe('cookieQueries', () => {
  it('reads unpartitioned cookies for a top-level navigation, with a first-party isolation fallback', () => {
    expect(
      cookieQueries('https://files.example.com/a.zip', 'firefox-container-1', {
        initiator: 'https://other.test/',
        navigation: true,
      }),
    ).toEqual([
      { url: 'https://files.example.com/a.zip', storeId: 'firefox-container-1' },
      {
        url: 'https://files.example.com/a.zip',
        storeId: 'firefox-container-1',
        firstPartyDomain: 'example.com',
      },
    ]);
  });

  it('reads the cookies partitioned under the tab site for media from another site', () => {
    const url = 'https://media.cdn.test/v.mp4';
    expect(
      cookieQueries(url, null, {
        initiator: 'https://www.example.co.uk/watch',
        topLevel: 'https://www.example.co.uk/watch',
        navigation: false,
      }),
    ).toEqual([
      { url, partitionKey: { topLevelSite: 'https://example.co.uk' } },
      {
        url,
        partitionKey: { topLevelSite: 'https://example.co.uk' },
        firstPartyDomain: 'example.co.uk',
      },
      { url },
      { url, firstPartyDomain: 'example.co.uk' },
    ]);
  });

  it('reads unpartitioned cookies for media from the same site', () => {
    const url = 'https://media.example.com/v.mp4';
    expect(
      cookieQueries(url, null, {
        initiator: 'https://www.example.com/watch',
        topLevel: 'https://www.example.com/watch',
        navigation: false,
      }),
    ).toEqual([{ url }, { url, firstPartyDomain: 'example.com' }]);
  });
});

describe('sites on the Public Suffix List', () => {
  it.each([
    // Private section: every customer has a site of their own.
    ['victim.myshopify.com', 'victim.myshopify.com'],
    ['cdn.victim.myshopify.com', 'victim.myshopify.com'],
    ['bucket.r2.dev', 'bucket.r2.dev'],
    ['bucket.s3.us-east-2.amazonaws.com', 'bucket.s3.us-east-2.amazonaws.com'],
    ['shop.example.uk.com', 'example.uk.com'],
    // ICANN section rules the old approximation got wrong.
    ['www.school.k12.ca.us', 'school.k12.ca.us'],
    ['www.example.ac.jp', 'example.ac.jp'],
    ['www.example.xn--p1ai', 'example.xn--p1ai'],
    // A host that is a public suffix itself is its own site.
    ['myshopify.com', 'myshopify.com'],
    ['co.uk', 'co.uk'],
    // No DNS name has an empty label, so no rule applies: the host is its own site.
    ['attacker..com', 'attacker..com'],
    ['a..example.com', 'a..example.com'],
  ])('the registrable domain of %s is %s', (host, domain) => {
    expect(registrableDomain(host)).toBe(domain);
  });

  it('keeps unrelated customers of one shared host on different sites', () => {
    const url = 'https://victim.myshopify.com/export';
    for (const initiator of [
      'https://attacker.myshopify.com/',
      'https://myshopify.com/',
      'https://victim.myshopify.com.evil.test/',
      'https://evilvictim.myshopify.com/',
    ]) {
      expect(isCrossSite(url, { initiator, navigation: true }), initiator).toBe(true);
    }
    expect(
      isCrossSite(url, { initiator: 'https://admin.victim.myshopify.com/', navigation: true }),
    ).toBe(false);
  });

  it('withholds Strict cookies from a link one shared-host customer gives to another', () => {
    const context: CookieContext = {
      initiator: 'https://attacker.myshopify.com/links',
      navigation: true,
    };
    expect(names(cookiesToSend(JAR, 'https://victim.myshopify.com/export', context))).toEqual([
      'none',
      'lax',
      'unspecified',
    ]);
  });

  it('reads partitioned cookies for media one shared-host customer loads from another', () => {
    const url = 'https://victim.myshopify.com/v.mp4';
    const context: CookieContext = {
      initiator: 'https://attacker.myshopify.com/watch',
      topLevel: 'https://attacker.myshopify.com/watch',
      navigation: false,
    };
    expect(names(cookiesToSend(JAR, url, context))).toEqual(['none']);
    expect(cookieQueries(url, null, context)[0]).toEqual({
      url,
      partitionKey: { topLevelSite: 'https://attacker.myshopify.com' },
    });
  });
});
