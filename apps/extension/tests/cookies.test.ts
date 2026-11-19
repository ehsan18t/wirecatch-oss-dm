import type { CookieRequest, SameSite } from '@wirecatch/protocol';
import { describe, expect, it } from 'vitest';
import {
  cookieReads,
  downloadCookies,
  mergeCookies,
  type CookieLike,
  type StoredCookie,
} from '../src/lib/cookies';

const SAME_SITES: SameSite[] = ['no_restriction', 'lax', 'strict', 'unspecified'];

/** Each cookie named after its SameSite value. */
const JAR: CookieLike[] = SAME_SITES.map((sameSite) => ({ name: sameSite, value: '1', sameSite }));

function names(cookies: CookieLike[]): string[] {
  return cookies.map((c) => c.name);
}

/** The sites the app names (D280), here for these addresses. */
function plan(
  site: string | null,
  topLevel: string | null,
): { site: string | null; top_level_site: string | null } {
  return { site, top_level_site: topLevel };
}

describe('cookieReads', () => {
  // Cloudflare's partitioned cf_clearance is read this way: without it the site answers 403.
  it('reads the cookies partitioned under the download site for a top-level navigation, then the unpartitioned ones', () => {
    const url = 'https://files.example.com/a.zip';
    const base = { url, storeId: 'firefox-container-1' };
    expect(
      cookieReads(url, 'firefox-container-1', plan('https://example.com', 'https://example.com')),
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
    expect(cookieReads(url, null, plan('https://cdn.test', 'https://example.co.uk'))).toEqual([
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
    expect(cookieReads(url, null, plan('https://example.com', 'https://example.com'))).toEqual([
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
    expect(cookieReads(url, null, plan('https://example.com', null))).toEqual([
      [{ url }, { url, firstPartyDomain: 'example.com' }],
    ]);
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
  it('sends the app every cookie the request can reach, with the request, and no header of its own choosing (D280)', () => {
    const request: CookieRequest = { initiator: 'https://other.test/links', navigation: true };
    const framed: CookieLike = {
      name: 'framed',
      value: '9',
      sameSite: 'no_restriction',
      partitionKey: { topLevelSite: 'https://example.com', hasCrossSiteAncestor: true },
    };
    const cookies = downloadCookies([...JAR, framed], request);
    expect(cookies.cookies).toBeNull();
    expect(cookies.cookie_jar).toEqual({
      cookies: SAME_SITES.map((sameSite) => ({ name: sameSite, value: '1', same_site: sameSite })),
      request,
    });
  });

  it('sends a cookie whose SameSite the browser does not report as SameSite=None', () => {
    const request: CookieRequest = { initiator: null, navigation: false };
    const cookies = downloadCookies([{ name: 'old', value: '1' }], request);
    expect(cookies.cookie_jar?.cookies).toEqual([
      { name: 'old', value: '1', same_site: 'no_restriction' },
    ]);
  });
});
