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
 * Second-level labels under a two-letter country code that act as public
 * suffixes (`example.co.uk`, `example.com.au`). An approximation of the Public
 * Suffix List, which the extension APIs do not expose.
 */
const COUNTRY_SECOND_LEVELS = new Set([
  'ac',
  'co',
  'com',
  'edu',
  'gob',
  'go',
  'gov',
  'gv',
  'ltd',
  'me',
  'mil',
  'ne',
  'net',
  'nic',
  'nom',
  'or',
  'org',
  'plc',
  'sch',
]);

/**
 * Shared hosting suffixes where every customer gets a site of their own
 * (`alice.github.io` and `bob.github.io` are different sites), from the private
 * part of the Public Suffix List.
 */
const SHARED_SUFFIXES = new Set([
  'appspot.com',
  'azurewebsites.net',
  'blogspot.com',
  'cloudfront.net',
  'firebaseapp.com',
  'fly.dev',
  'github.io',
  'gitlab.io',
  'glitch.me',
  'herokuapp.com',
  'netlify.app',
  'ngrok.io',
  'onrender.com',
  'pages.dev',
  's3.amazonaws.com',
  'vercel.app',
  'web.app',
  'workers.dev',
]);

/**
 * The registrable domain of a host (`www.example.co.uk` gives `example.co.uk`),
 * approximated without the full Public Suffix List. IP addresses and single
 * labels are their own site.
 */
export function registrableDomain(host: string): string {
  const h = host.toLowerCase().replace(/\.+$/, '');
  if (h.startsWith('[') || /^\d+(\.\d+){3}$/.test(h)) return h;
  const labels = h.split('.');
  if (labels.length <= 2) return h;
  return labels.slice(-(suffixLabels(labels) + 1)).join('.');
}

/** Labels in the longest of `SHARED_SUFFIXES`. */
const MAX_SHARED_SUFFIX_LABELS = Math.max(...[...SHARED_SUFFIXES].map((s) => s.split('.').length));

/** Country code top-level domains have two letters (`uk`, `au`). */
const COUNTRY_CODE_LENGTH = 2;

/** How many of the host's last labels are its public suffix (`co.uk` is two, `com` is one). */
function suffixLabels(labels: readonly string[]): number {
  for (let n = Math.min(MAX_SHARED_SUFFIX_LABELS, labels.length); n > 1; n -= 1) {
    if (SHARED_SUFFIXES.has(labels.slice(-n).join('.'))) return n;
  }
  const top = labels[labels.length - 1] ?? '';
  const second = labels[labels.length - 2] ?? '';
  return top.length === COUNTRY_CODE_LENGTH && COUNTRY_SECOND_LEVELS.has(second) ? 2 : 1;
}

/** The site of a web address, with its scheme (`https://example.com`), or null for anything else. */
export function siteOf(url: string | null): string | null {
  if (url === null) return null;
  try {
    const u = new URL(url);
    if ((u.protocol !== 'http:' && u.protocol !== 'https:') || u.hostname === '') return null;
    return `${u.protocol}//${registrableDomain(u.hostname)}`;
  } catch {
    return null;
  }
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
