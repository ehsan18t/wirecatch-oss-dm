import {
  isSameSite,
  type CookiePlan,
  type CookieRequest,
  type NewDownload,
} from '@wirecatch/protocol';

/** The fields of a `cookies.Cookie` needed to build a `Cookie` header. */
export interface CookieLike {
  name: string;
  value: string;
  /** `no_restriction` (SameSite=None), `lax`, `strict`, or `unspecified` (no attribute). */
  sameSite?: string;
  /** Present on partitioned cookies only; `hasCrossSiteAncestor` from Chromium 130. */
  partitionKey?: { topLevelSite?: string; hasCrossSiteAncestor?: boolean };
}

/** The cookie store a request belongs to: Firefox names it (a container, or its private store); Chromium names none, so `private` says which. */
export interface CookieStore {
  id: string | null;
  private: boolean;
}

/** The store of a tab or download, from what the browser says about it. */
export function cookieStoreOf(source: {
  incognito?: boolean;
  cookieStoreId?: string | null;
}): CookieStore {
  return { id: source.cookieStoreId ?? null, private: source.incognito === true };
}

/** What to pass to `cookies.getAll`. `firstPartyDomain` is Firefox only (first-party isolation). */
export interface CookieQuery {
  url: string;
  storeId?: string;
  partitionKey?: { topLevelSite: string };
  firstPartyDomain?: string;
}

/** A top-level navigation has no frame above it, so it never gets cookies partitioned for a frame with a cross-site ancestor. */
function reachedBy<T extends CookieLike>(cookies: readonly T[], request: CookieRequest): T[] {
  return cookies.filter(
    (c) => !request.navigation || c.partitionKey?.hasCrossSiteAncestor !== true,
  );
}

/** The cookies sent with a download: the browser's whole jar for its address, from which the app picks with its own Public Suffix List (D112, D279). `cookies`, the header itself, goes only with media whose own request was seen. */
export type DownloadCookies = Pick<NewDownload, 'cookies' | 'cookie_jar'>;

export const NO_COOKIES: DownloadCookies = { cookies: null };

export function downloadCookies(
  cookies: readonly CookieLike[],
  request: CookieRequest,
): DownloadCookies {
  return {
    cookies: null,
    cookie_jar: {
      cookies: reachedBy(cookies, request).map((c) => ({
        name: c.name,
        value: c.value,
        // Kept as SameSite=None: the app reads no other value for a cookie without the attribute.
        same_site: isSameSite(c.sameSite) ? c.sameSite : 'no_restriction',
      })),
      request,
    },
  };
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
  plan: Pick<CookiePlan, 'site' | 'top_level_site'>,
): CookieQuery[][] {
  const base: CookieQuery = storeId === null ? { url } : { url, storeId };
  // The app names the sites with its own Public Suffix List, so the extension carries none (D279).
  const site = plan.site;
  const topLevel = plan.top_level_site;
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
