/**
 * Host name helpers for the "sites to ignore" setting.
 *
 * A site entry such as `example.com` covers that host and every subdomain of it
 * (`www.example.com`, `cdn.example.com`), but not look-alikes such as `badexample.com`.
 * The app keeps each entry the way `normalizeSite` from `@wirecatch/protocol` does.
 */
import { normalizeSite } from '@wirecatch/protocol';

/** The host name of a URL in lower case, or null if the URL has none. */
export function hostOf(url: string): string | null {
  try {
    const host = new URL(url).hostname.toLowerCase();
    return host === '' ? null : host;
  } catch {
    return null;
  }
}

/** The site to ignore when the person clicks "Ignore this site" on a page: its host without `www.`. */
export function siteForPage(url: string | null): string | null {
  if (url === null) return null;
  if (!/^https?:/i.test(url)) return null;
  const host = hostOf(url);
  if (host === null) return null;
  return host.replace(/^www\./, '');
}

/** True when `host` is `site` or one of its subdomains. */
export function hostMatchesSite(host: string, site: string): boolean {
  return host === site || host.endsWith(`.${site}`);
}

/** The ignored-site entry that covers `url`, if any. */
export function matchingSite(url: string | null, sites: readonly string[]): string | null {
  if (url === null) return null;
  const host = hostOf(url);
  if (host === null) return null;
  for (const entry of sites) {
    const site = normalizeSite(entry);
    if (site !== null && hostMatchesSite(host, site)) return entry;
  }
  return null;
}
