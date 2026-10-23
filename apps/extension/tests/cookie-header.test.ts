/** The `Cookie` header for a request, read from a browser's cookie store the way each browser answers. */
import { describe, expect, it } from 'vitest';
import { cookieHeaderFor } from '../src/lib/browser/adapters';
import type { CookieContext, CookieQuery, StoredCookie } from '../src/lib/cookies';
import { stubGetAll } from './fakes';

const SITE = 'https://downloads.test';

const cookie = (name: string, value: string, more: Partial<StoredCookie> = {}): StoredCookie => ({
  name,
  value,
  domain: '.downloads.test',
  path: '/',
  sameSite: 'no_restriction',
  ...more,
});

/**
 * One site's cookies: a sign-in, a preference, a partitioned clearance like
 * Cloudflare's, one partitioned while framed through another site, and one set
 * while embedded in another site.
 */
const STORE: StoredCookie[] = [
  cookie('uid', '1', { sameSite: 'lax' }),
  cookie('pref', '2'),
  cookie('cf_clearance', '3', { partitionKey: { topLevelSite: SITE } }),
  cookie('framed', '4', { partitionKey: { topLevelSite: SITE, hasCrossSiteAncestor: true } }),
  cookie('embedded', '5', { partitionKey: { topLevelSite: 'https://news.test' } }),
];

/**
 * What `cookies.getAll` answers in Chromium and Firefox: without `partitionKey`
 * only the unpartitioned cookies, with one only that partition.
 */
function getAll(query: CookieQuery): StoredCookie[] {
  const partition = query.partitionKey?.topLevelSite;
  return STORE.filter((c) => c.partitionKey?.topLevelSite === partition);
}

const BROWSERS = {
  'Chromium and Firefox': getAll,
  'Firefox with first-party isolation': (query: CookieQuery) => {
    if (query.firstPartyDomain === undefined) throw new Error('firstPartyDomain is required');
    return getAll(query);
  },
};

/** The cookies of a header, in any order. */
function sent(header: string | null): string[] {
  return header === null ? [] : header.split('; ').sort();
}

describe.each(Object.entries(BROWSERS))('cookieHeaderFor in %s', (_, answer) => {
  it('sends a download link the cookies partitioned under its site, as the browser does', async () => {
    stubGetAll(answer);
    const context: CookieContext = { initiator: `${SITE}/details?id=1`, navigation: true };
    expect(sent(await cookieHeaderFor(`${SITE}/download?id=1`, null, context))).toEqual([
      'cf_clearance=3',
      'pref=2',
      'uid=1',
    ]);
  });

  it('sends media from another site only the cookies of its partition', async () => {
    stubGetAll(answer);
    const context: CookieContext = {
      initiator: 'https://news.test/watch',
      topLevel: 'https://news.test/watch',
      navigation: false,
    };
    expect(sent(await cookieHeaderFor(`${SITE}/v.mp4`, null, context))).toEqual(['embedded=5']);
  });
});
