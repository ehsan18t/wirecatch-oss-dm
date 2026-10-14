import { getDomain } from 'tldts';

import { parseUrl } from './url';

/** The fields of a `cookies.Cookie` needed to build a `Cookie` header. */
export interface CookieLike {
  name: string;
  value: string;
  /** `no_restriction` (SameSite=None), `lax`, `strict`, or `unspecified` (no attribute). */
  sameSite?: string;
}

/** `[{name: "a", value: "1"}, {name: "b", value: "2"}]` becomes `"a=1; b=2"`; no cookies gives null. */
export function cookieHeader(cookies: readonly CookieLike[]): string | null {
  if (cookies.length === 0) return null;
  return cookies.map((c) => (c.name === '' ? c.value : `${c.name}=${c.value}`)).join('; ');
}

/**
 * Where a request for a URL comes from, so Wirecatch sends only the cookies the
 * browser itself would send with it.
 */
export interface CookieContext {
  /** The page or frame that started the request; null when the browser does not say. */
  initiator: string | null;
  /** The address before redirects, when it differs from the URL. */
  originalUrl?: string | null;
  /**
   * The tab's page, for requests a page makes itself (media). Null for top-level
   * navigations, whose own address is the top-level site.
   */
  topLevel?: string | null;
  /**
   * True when the browser would load the URL as a top-level GET navigation (a
   * download started from a link, the context menu, a page address); false for
   * media a page loads itself.
   */
  navigation: boolean;
}

/** What to pass to `cookies.getAll`. `firstPartyDomain` is Firefox only (first-party isolation). */
export interface CookieQuery {
  url: string;
  storeId?: string;
  partitionKey?: { topLevelSite: string };
  firstPartyDomain?: string;
}

/**
 * Both sections of the Public Suffix List: shared hosts such as `myshopify.com` give
 * every customer a site of their own. The list is the one bundled in `tldts`, which
 * must stay current (D35).
 */
const PSL_OPTIONS = { allowPrivateDomains: true, extractHostname: false } as const;

/**
 * The registrable domain of a host (`www.example.co.uk` gives `example.co.uk`),
 * from the Public Suffix List, as the browser decides what is same-site. IP
 * addresses, single labels, hosts with an empty label and hosts that are a
 * public suffix themselves are their own site.
 */
export function registrableDomain(host: string): string {
  const h = host.toLowerCase().replace(/\.+$/, '');
  if (h.split('.').includes('')) return h;
  return getDomain(h, PSL_OPTIONS) ?? h;
}

/** The site of a web address, with its scheme (`https://example.com`), or null for anything else. */
export function siteOf(url: string | null): string | null {
  const u = url === null ? null : parseUrl(url);
  if (u === null || (u.protocol !== 'http:' && u.protocol !== 'https:') || u.hostname === '') {
    return null;
  }
  return `${u.protocol}//${registrableDomain(u.hostname)}`;
}

/**
 * True when the browser would treat the request as cross-site: the page that
 * started it, an address it was redirected from, or the tab's page is on another
 * site (scheme included, as Chromium compares them). An unknown starting page
 * counts as cross-site, so a page that hides its address gains nothing by it.
 */
export function isCrossSite(url: string, context: CookieContext): boolean {
  const target = siteOf(url);
  if (target === null) return true;
  const initiator = siteOf(context.initiator);
  if (initiator !== target) return true;
  const originalUrl = context.originalUrl ?? null;
  if (originalUrl !== null && siteOf(originalUrl) !== target) return true;
  const topLevel = context.topLevel ?? null;
  return topLevel !== null && siteOf(topLevel) !== target;
}

/**
 * The cookies the browser would send with this request, from what `cookies.getAll`
 * returned for its address. Same-site: all of them. Cross-site: never
 * `SameSite=Strict`; `Lax` (and cookies without the attribute, which Chromium
 * treats as Lax) only for a top-level navigation; `SameSite=None` always.
 */
export function cookiesToSend<T extends CookieLike>(
  cookies: readonly T[],
  url: string,
  context: CookieContext,
): T[] {
  if (!isCrossSite(url, context)) return [...cookies];
  return cookies.filter((c) => {
    if (c.sameSite === 'strict') return false;
    if (c.sameSite === 'lax' || c.sameSite === 'unspecified') return context.navigation;
    return true;
  });
}

/**
 * The `cookies.getAll` queries to try, in order, until one succeeds.
 *
 * Media a page loads from another site reads the cookies partitioned under the
 * tab's site (`partitionKey`, Chromium and Firefox), as the browser does for a
 * third-party request; top-level navigations and same-site requests read the
 * normal, unpartitioned cookies. Firefox with first-party isolation refuses a
 * query without `firstPartyDomain`, so a second query adds it; browsers too old
 * to know `partitionKey` reject it, so the last queries leave it out.
 */
export function cookieQueries(
  url: string,
  storeId: string | null,
  context: CookieContext,
): CookieQuery[] {
  const base: CookieQuery = storeId === null ? { url } : { url, storeId };
  const topLevel = context.navigation ? null : siteOf(context.topLevel ?? null);
  const firstParty = topLevel ?? siteOf(url);
  const firstPartyDomain = firstParty === null ? null : firstParty.replace(/^https?:\/\//, '');
  const partitioned = topLevel !== null && topLevel !== siteOf(url);

  const queries: CookieQuery[] = [];
  const add = (query: CookieQuery): void => {
    queries.push(query);
    if (firstPartyDomain !== null) queries.push({ ...query, firstPartyDomain });
  };
  if (partitioned) add({ ...base, partitionKey: { topLevelSite: topLevel } });
  add(base);
  return queries;
}
