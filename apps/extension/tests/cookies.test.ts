import type { CookieRequest, SameSite } from '@wirecatch/protocol';
import { describe, expect, it } from 'vitest';
import {
  cookieReads,
  cookiesToSend,
  downloadCookies,
  isCrossSite,
  mergeCookies,
  siteOf,
  type CookieLike,
  type StoredCookie,
} from '../src/lib/cookies';
import SHARED from '../../../packages/protocol/src/cookie-scope.cases.json';

/** The cases the app's cookie scope is tested against too (D112). */
interface Cases {
  sites: [string, string | null][];
  jar: SameSite[];
  requests: { why: string; url: string; request: CookieRequest; sent: SameSite[] }[];
}

// JSON reads as plain strings, wider than the protocol's types.
const CASES = SHARED as unknown as Cases;

/** Each cookie named after its SameSite value, as the cases name them. */
const JAR: CookieLike[] = CASES.jar.map((sameSite) => ({ name: sameSite, value: '1', sameSite }));

function names(cookies: CookieLike[]): string[] {
  return cookies.map((c) => c.name);
}

describe('sites', () => {
  it.each(CASES.sites)('the site of %s is %s', (address, site) => {
    expect(siteOf(address)).toBe(site);
  });

  it('has no site for a missing address', () => {
    expect(siteOf(null)).toBeNull();
  });
});

describe('cookiesToSend', () => {
  const url = 'https://files.example.com/report.pdf';

  it.each(CASES.requests.map((c) => [c.why, c] as const))('%s', (_, c) => {
    expect(names(cookiesToSend(JAR, c.url, c.request))).toEqual(c.sent);
    expect(isCrossSite(c.url, c.request)).toBe(c.sent.length < JAR.length);
  });

  it('never sends a top-level navigation the cookies partitioned for a frame with a cross-site ancestor', () => {
    const partitioned = (name: string, hasCrossSiteAncestor?: boolean): CookieLike => ({
      name,
      value: name,
      sameSite: 'no_restriction',
      partitionKey: { topLevelSite: 'https://example.com', hasCrossSiteAncestor },
    });
    const jar = [partitioned('top', false), partitioned('framed', true), partitioned('older')];
    const context: CookieRequest = { initiator: 'https://example.com/', navigation: true };
    expect(names(cookiesToSend(jar, url, context))).toEqual(['top', 'older']);
  });

  it('keeps cookies whose SameSite state the browser does not report', () => {
    const cookies = [{ name: 'old', value: '1' }];
    expect(
      cookiesToSend(cookies, url, { initiator: 'https://other.test/', navigation: false }),
    ).toEqual(cookies);
  });
});

describe('cookieReads', () => {
  // Cloudflare's partitioned cf_clearance is read this way: without it the site answers 403.
  it('reads the cookies partitioned under the download site for a top-level navigation, then the unpartitioned ones', () => {
    const url = 'https://files.example.com/a.zip';
    const base = { url, storeId: 'firefox-container-1' };
    expect(
      cookieReads(url, 'firefox-container-1', {
        initiator: 'https://other.test/',
        navigation: true,
      }),
    ).toEqual([
      [
        { ...base, partitionKey: { topLevelSite: 'https://example.com' } },
        {
          ...base,
          partitionKey: { topLevelSite: 'https://example.com' },
          firstPartyDomain: 'example.com',
        },
      ],
      [base, { ...base, firstPartyDomain: 'example.com' }],
    ]);
  });

  it('reads only the cookies partitioned under the tab site for media from another site, unless the browser knows no partitions', () => {
    const url = 'https://media.cdn.test/v.mp4';
    expect(
      cookieReads(url, null, {
        initiator: 'https://www.example.co.uk/watch',
        top_level: 'https://www.example.co.uk/watch',
        navigation: false,
      }),
    ).toEqual([
      [
        { url, partitionKey: { topLevelSite: 'https://example.co.uk' } },
        {
          url,
          partitionKey: { topLevelSite: 'https://example.co.uk' },
          firstPartyDomain: 'example.co.uk',
        },
        { url },
        { url, firstPartyDomain: 'example.co.uk' },
      ],
    ]);
  });

  it('reads the cookies partitioned under the tab site for media from the same site', () => {
    const url = 'https://media.example.com/v.mp4';
    expect(
      cookieReads(url, null, {
        initiator: 'https://www.example.com/watch',
        top_level: 'https://www.example.com/watch',
        navigation: false,
      }),
    ).toEqual([
      [
        { url, partitionKey: { topLevelSite: 'https://example.com' } },
        {
          url,
          partitionKey: { topLevelSite: 'https://example.com' },
          firstPartyDomain: 'example.com',
        },
      ],
      [{ url }, { url, firstPartyDomain: 'example.com' }],
    ]);
  });

  it('reads only unpartitioned cookies for media whose tab is unknown', () => {
    const url = 'https://media.example.com/v.mp4';
    expect(cookieReads(url, null, { initiator: null, top_level: null, navigation: false })).toEqual(
      [[{ url }, { url, firstPartyDomain: 'example.com' }]],
    );
  });
});

describe('mergeCookies', () => {
  const cookie = (name: string, path: string, topLevelSite?: string): StoredCookie => ({
    name,
    value: name,
    domain: '.example.com',
    path,
    ...(topLevelSite === undefined ? {} : { partitionKey: { topLevelSite } }),
  });

  it('keeps each cookie once when two reads return it', () => {
    const sid = cookie('sid', '/');
    const clearance = cookie('cf_clearance', '/', 'https://example.com');
    expect(mergeCookies([[sid, clearance], [sid]])).toEqual([sid, clearance]);
  });

  it('keeps a partitioned and an unpartitioned cookie of one name, as the browser sends both', () => {
    const plain = cookie('id', '/');
    const partitioned = cookie('id', '/', 'https://example.com');
    const merged = mergeCookies([[partitioned], [plain]]);
    expect(merged).toHaveLength(2);
    expect(merged).toEqual(expect.arrayContaining([partitioned, plain]));
  });

  it('puts cookies with longer paths first', () => {
    const root = cookie('a', '/');
    const deep = cookie('b', '/files/');
    expect(names(mergeCookies([[root], [deep]]))).toEqual(['b', 'a']);
  });
});

describe('downloadCookies', () => {
  it('sends the app every cookie the request can reach, with the request, beside the header', () => {
    const request: CookieRequest = { initiator: 'https://other.test/links', navigation: true };
    const framed: CookieLike = {
      name: 'framed',
      value: '9',
      sameSite: 'no_restriction',
      partitionKey: { topLevelSite: 'https://example.com', hasCrossSiteAncestor: true },
    };
    const cookies = downloadCookies([...JAR, framed], 'https://files.example.com/a.zip', request);
    expect(cookies.cookies).toBe('no_restriction=1; lax=1; unspecified=1');
    expect(cookies.cookie_jar).toEqual({
      cookies: CASES.jar.map((sameSite) => ({ name: sameSite, value: '1', same_site: sameSite })),
      request,
    });
  });

  it('sends a cookie whose SameSite the browser does not report as SameSite=None', () => {
    const request: CookieRequest = { initiator: null, navigation: false };
    const cookies = downloadCookies([{ name: 'old', value: '1' }], 'https://example.com/', request);
    expect(cookies.cookie_jar?.cookies).toEqual([
      { name: 'old', value: '1', same_site: 'no_restriction' },
    ]);
  });
});

describe('sites on the Public Suffix List', () => {
  it('reads partitioned cookies for media one shared-host customer loads from another', () => {
    const url = 'https://victim.myshopify.com/v.mp4';
    const context: CookieRequest = {
      initiator: 'https://attacker.myshopify.com/watch',
      top_level: 'https://attacker.myshopify.com/watch',
      navigation: false,
    };
    expect(names(cookiesToSend(JAR, url, context))).toEqual(['no_restriction']);
    expect(cookieReads(url, null, context)[0]?.[0]).toEqual({
      url,
      partitionKey: { topLevelSite: 'https://attacker.myshopify.com' },
    });
  });
});
