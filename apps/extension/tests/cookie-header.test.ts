/** The `Cookie` header for a request, read from a browser's cookie store the way each browser answers. */
import type { CookieRequest } from '@wirecatch/protocol';
import { describe, expect, it, vi } from 'vitest';
import { browser } from 'wxt/browser';
import { cookiesFor } from '../src/lib/browser/adapters';
import type { CookieQuery, StoredCookie } from '../src/lib/cookies';
import { NORMAL_STORE, stubCookieStores, stubGetAll } from './fakes';

/** The header picked for an app before protocol version 2. */
async function header(...args: Parameters<typeof cookiesFor>): Promise<string | null> {
  return (await cookiesFor(...args)).cookies;
}

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

describe.each(Object.entries(BROWSERS))('the cookie header in %s', (_, answer) => {
  it('sends a download link the cookies partitioned under its site, as the browser does', async () => {
    stubGetAll(answer);
    const context: CookieRequest = { initiator: `${SITE}/details?id=1`, navigation: true };
    expect(sent(await header(`${SITE}/download?id=1`, NORMAL_STORE, context))).toEqual([
      'cf_clearance=3',
      'pref=2',
      'uid=1',
    ]);
  });

  it('sends media from another site only the cookies of its partition', async () => {
    stubGetAll(answer);
    const context: CookieRequest = {
      initiator: 'https://news.test/watch',
      top_level: 'https://news.test/watch',
      navigation: false,
    };
    expect(sent(await header(`${SITE}/v.mp4`, NORMAL_STORE, context))).toEqual(['embedded=5']);
  });
});

describe('the cookie header in a private window (D82)', () => {
  const context: CookieRequest = { initiator: null, navigation: true };
  const PRIVATE_CHROMIUM = { id: null, private: true };

  /** One sign-in per store, named after the store, so the header tells which store was read. */
  function stubStores(): (string | undefined)[] {
    const reads: (string | undefined)[] = [];
    stubGetAll((query) => {
      reads.push(query.storeId);
      return [cookie('uid', query.storeId ?? 'default')];
    });
    return reads;
  }

  it("reads the Chromium store that holds the private tabs, never the normal window's", async () => {
    const reads = stubStores();
    stubCookieStores(
      [
        { id: '0', tabIds: [1] },
        { id: '1', tabIds: [2] },
      ],
      [
        { id: 1, incognito: false },
        { id: 2, incognito: true },
      ],
    );
    expect(await header(`${SITE}/a.zip`, PRIVATE_CHROMIUM, context)).toBe('uid=1');
    expect(new Set(reads)).toEqual(new Set(['1']));
  });

  it('sends no cookies when no store holds a private tab, or the stores cannot be read', async () => {
    const reads = stubStores();
    stubCookieStores([{ id: '0', tabIds: [1] }], [{ id: 1, incognito: false }]);
    expect(await header(`${SITE}/a.zip`, PRIVATE_CHROMIUM, context)).toBeNull();
    vi.spyOn(browser.cookies, 'getAllCookieStores').mockRejectedValue(new Error('no stores'));
    expect(await header(`${SITE}/a.zip`, PRIVATE_CHROMIUM, context)).toBeNull();
    expect(reads).toEqual([]);
  });

  it('reads the store Firefox names without looking for one', async () => {
    const reads = stubStores();
    const stores = vi.spyOn(browser.cookies, 'getAllCookieStores');
    const store = { id: 'firefox-private', private: true };
    expect(await header(`${SITE}/a.zip`, store, context)).toBe('uid=firefox-private');
    expect(new Set(reads)).toEqual(new Set(['firefox-private']));
    expect(stores).not.toHaveBeenCalled();
  });

  it('reads the default store for a normal window', async () => {
    const reads = stubStores();
    expect(await header(`${SITE}/a.zip`, NORMAL_STORE, context)).toBe('uid=default');
    expect(new Set(reads)).toEqual(new Set([undefined]));
  });
});
