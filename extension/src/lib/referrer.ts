import { parseUrl, parseWebUrl } from './url';

/**
 * The `Referer` a browser sends by default (the `strict-origin-when-cross-origin`
 * policy) when `page` asks for `target`:
 *
 * - same origin: the full page address, without its fragment or any user name and password;
 * - another origin: only the page's origin (`https://example.com/`);
 * - from https to anything that is not https: nothing, so the address never leaves in the clear.
 *
 * Returns null when there is no page, or it is not a web page.
 */
export function refererFor(target: string, page: string | null): string | null {
  const from = page === null ? null : parseWebUrl(page);
  const to = parseUrl(target);
  if (from === null || to === null) return null;
  if (from.protocol === 'https:' && to.protocol !== 'https:') return null;
  if (from.origin !== to.origin) return `${from.origin}/`;
  from.hash = '';
  from.username = '';
  from.password = '';
  return from.href;
}
