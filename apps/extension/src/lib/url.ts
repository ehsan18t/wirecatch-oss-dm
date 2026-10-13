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
