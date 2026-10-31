/**
 * `text` read as an absolute address, or null when it is not one. Every address the extension
 * reads comes from a page, a tab or the browser, so one that does not parse is an answer ("not a
 * web address"), not a failure, and every caller has its own answer for it.
 */
export function parseUrl(text: string): URL | null {
  try {
    return new URL(text);
  } catch {
    // Not an absolute address: the null says so, and is not an error to report.
    return null;
  }
}

/** `text` read as an http or https address with a host, or null for anything else. */
export function parseWebUrl(text: string): URL | null {
  const url = parseUrl(text);
  if (url === null || (url.protocol !== 'http:' && url.protocol !== 'https:')) return null;
  return url.hostname === '' ? null : url;
}

/** True for http and https addresses; false for `blob:`, `data:`, `mediasource:` and anything else. */
export function isWebUrl(text: string): boolean {
  return parseWebUrl(text) !== null;
}

/** The host name of an address in lower case, or null if it has none. */
export function hostOf(url: string): string | null {
  const host = parseUrl(url)?.hostname.toLowerCase() ?? '';
  return host === '' ? null : host;
}
