// Sites and file types the way the app keeps them: ports of `normalize_site` and `normalize_file_type` in crates/protocol/src/extension.rs, tested against the cases written from there (`generated/fixtures.json`), so both languages agree.
import { decodePunycode, NOT_ASCII } from './punycode';

/** White space as Rust's `char::is_whitespace` and `str::trim` have it (the Unicode White_Space property), which `String.prototype.trim` and `\s` do not: they take U+FEFF and leave U+0085. */
const WHITE_SPACE = /\p{White_Space}/u;
const SURROUNDING_WHITE_SPACE = /^\p{White_Space}+|\p{White_Space}+$/gu;
/** A URL scheme and the `://` after it. Only a scheme ends there, never a path or query that holds another address. */
const SCHEME = /^[a-z][a-z\d+.-]*:\/\//i;
/** Any number of leading `*.` and `.`. */
const LEADING_WILDCARDS = /^(?:\*?\.)+/;

/** The part of `text` before any path, query or fragment. */
function beforePath(text: string): string {
  return text.split(/[/\\?#]/, 1)[0] ?? '';
}

/** The scheme an address is parsed with to read its host: one whose host the URL standard parses as a browser does. */
const HOST_SCHEME = 'http://';

/** What `URL` makes of `host`, or null when it is no host. */
function urlHost(host: string): string | null {
  try {
    return new URL(`${HOST_SCHEME}${host}`).hostname;
  } catch {
    return null;
  }
}

/**
 * Whether an `xn--` label of a parsed host is one the URL standard keeps: its Punycode decodes, to a name beyond ASCII that encodes back to the same label. Runtimes differ here (Node 24.22 and current Chromium keep any `xn--` label as it is, D68), so the label is checked rather than left to `URL`; turning the decoded Unicode name back into ASCII is the direction every runtime still checks.
 */
function isPunycodeLabel(label: string): boolean {
  const decoded = decodePunycode(label.slice('xn--'.length));
  return decoded !== null && NOT_ASCII.test(decoded) && urlHost(decoded) === label;
}

/**
 * The host the URL standard's host parser makes of `site` (IDNA, IPv4 and IPv6 forms, the characters no host may hold), as `url::Host::parse` does in Rust; null when it is no host. A colon is only allowed inside an IPv6 literal: elsewhere `URL` would read it as a port, where the host parser refuses it.
 */
function parsedHost(site: string): string | null {
  const literal = site.startsWith('[') && site.endsWith(']');
  if (!literal && site.includes(':')) return null;
  const host = urlHost(site);
  if (host === null || literal) return host;
  return host.split('.').every((label) => !label.startsWith('xn--') || isPunycodeLabel(label))
    ? host
    : null;
}

/**
 * The host name an ignored site stands for, the way the app keeps it: the host as a browser reports it (lower case, ASCII, a Unicode name in its punycode form, an IP address in its usual form), without a scheme, user, port, path, query, leading `*.` or `.`, or trailing dot. So `https://Bücher.de/page`, `*.bücher.de`, `.bücher.de` and `xn--bcher-kva.de.` are all `xn--bcher-kva.de`. Null when no host is left, or what is left is not a host name; that includes a `*` anywhere else, which no host a browser reports holds.
 */
export function normalizeSite(input: string): string | null {
  let site = input.replaceAll(SURROUNDING_WHITE_SPACE, '').replace(SCHEME, '');
  site = beforePath(site);
  site = site.slice(site.lastIndexOf('@') + 1);
  site = site.replace(LEADING_WILDCARDS, '');
  site = site.replace(/:\d*$/, '');
  if (site === '' || WHITE_SPACE.test(site) || site.includes('*')) return null;
  const host = parsedHost(site)?.replace(/\.+$/, '') ?? '';
  return host === '' ? null : host;
}

/** A file type the way the app keeps it in its lists (`normalize_file_type`): lower case, without white space around it or the leading `*.` or `.` a person may type, so `*.ZIP`, `.zip` and `zip` are all `zip`. Null when nothing is left. */
export function normalizeFileType(input: string): string | null {
  const type = input.replaceAll(SURROUNDING_WHITE_SPACE, '').replace(LEADING_WILDCARDS, '');
  return type === '' ? null : type.toLowerCase();
}
