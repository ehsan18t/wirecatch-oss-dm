import { getDomain } from 'tldts';
import { parseUrl } from './url';

/** The fields of a `cookies.Cookie` needed to build a `Cookie` header. */
export interface CookieLike {
  name: string;
  value: string;
  /** `no_restriction` (SameSite=None), `lax`, `strict`, or `unspecified` (no attribute). */
  sameSite?: string;
  /** Present on partitioned cookies only; `hasCrossSiteAncestor` from Chromium 130. */
  partitionKey?: { topLevelSite?: string; hasCrossSiteAncestor?: boolean };
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
 * treats as Lax) only for a top-level navigation; `SameSite=None` always. A
 * top-level navigation has no frame above it, so it never gets cookies partitioned
 * for a frame with a cross-site ancestor.
 */
export function cookiesToSend<T extends CookieLike>(
  cookies: readonly T[],
  url: string,
  context: CookieContext,
): T[] {
  const crossSite = isCrossSite(url, context);
  return cookies.filter((c) => {
    if (context.navigation && c.partitionKey?.hasCrossSiteAncestor === true) return false;
    if (!crossSite) return true;
    if (c.sameSite === 'strict') return false;
    if (c.sameSite === 'lax' || c.sameSite === 'unspecified') return context.navigation;
    return true;
  });
}

/** What `cookies.getAll` returns for one cookie, as far as telling cookies apart needs. */
export interface StoredCookie extends CookieLike {
  domain: string;
  path: string;
  /** Firefox only (first-party isolation). */
  firstPartyDomain?: string;
}

/**
 * The reads that together give the cookies the browser would send with a request.
 * Each read lists the `cookies.getAll` queries to try, in order, until one succeeds.
 *
 * `cookies.getAll` returns partitioned cookies only when asked for their partition,
 * and then only those (Cloudflare's `cf_clearance` is one). A request to the site of
 * its top-level page (a navigation's own address, or the tab's page for media) gets
 * the cookies partitioned under that site and the unpartitioned ones, so it takes
 * two reads. Media from another site gets only its partition, as Firefox sends it by
 * default. Chromium also sends the unpartitioned cookies while third-party cookies
 * are allowed, and Firefox once the site was granted storage access, which no read
 * can tell; media whose own request was seen therefore goes with the `Cookie` header
 * that request carried (`MediaItem.sentCookies`, D59) instead. The unpartitioned
 * queries there stand in for a browser too old to know `partitionKey`. When the tab is unknown there is no
 * partition to read. Firefox with first-party isolation refuses a query without
 * `firstPartyDomain`, so a second query adds it.
 */
export function cookieReads(
  url: string,
  storeId: string | null,
  context: CookieContext,
): CookieQuery[][] {
  const base: CookieQuery = storeId === null ? { url } : { url, storeId };
  const site = siteOf(url);
  const topLevel = context.navigation ? site : siteOf(context.topLevel ?? null);
  const firstParty = topLevel ?? site;
  const firstPartyDomain = firstParty === null ? null : firstParty.replace(/^https?:\/\//, '');
  const read = (query: CookieQuery): CookieQuery[] =>
    firstPartyDomain === null ? [query] : [query, { ...query, firstPartyDomain }];

  const unpartitioned = read(base);
  if (topLevel === null) return [unpartitioned];
  const partition = read({ ...base, partitionKey: { topLevelSite: topLevel } });
  if (topLevel !== site) return [[...partition, ...unpartitioned]];
  return [partition, unpartitioned];
}

/**
 * The cookies of several reads as one list, each cookie once, longer paths first as
 * browsers send them. The same cookie can come back from more than one read. A
 * partitioned and an unpartitioned cookie of one name are different cookies, and
 * the browser sends both.
 */
export function mergeCookies<T extends StoredCookie>(reads: readonly (readonly T[])[]): T[] {
  const seen = new Set<string>();
  const merged = reads.flat().filter((c) => {
    const identity = JSON.stringify([
      c.name,
      c.domain,
      c.path,
      c.partitionKey?.topLevelSite ?? null,
      c.partitionKey?.hasCrossSiteAncestor ?? null,
      c.firstPartyDomain ?? null,
    ]);
    if (seen.has(identity)) return false;
    seen.add(identity);
    return true;
  });
  return merged.sort((a, b) => b.path.length - a.path.length);
}
