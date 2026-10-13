// The extension's settings as the app keeps them, for the apps to show a change before the app confirms it: ports of `normalize_site`, `normalize_file_type` and `ExtensionSettings::apply` in crates/protocol/src/extension.rs, tested against the cases written from there (`generated/fixtures.json`), so both languages agree.
import type { ExtensionSettings } from './generated/ExtensionSettings';
import type { ExtensionSettingsPatch } from './generated/ExtensionSettingsPatch';

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

/**
 * The host the URL standard's host parser makes of `site` (IDNA, IPv4 and IPv6 forms, the characters no host may hold), as `url::Host::parse` does in Rust; null when it is no host. A colon is only allowed inside an IPv6 literal: elsewhere `URL` would read it as a port, where the host parser refuses it.
 */
function parsedHost(site: string): string | null {
  const literal = site.startsWith('[') && site.endsWith(']');
  if (!literal && site.includes(':')) return null;
  try {
    return new URL(`${HOST_SCHEME}${site}`).hostname;
  } catch {
    return null;
  }
}

/**
 * The host name an ignored site stands for, the way the app keeps it: the host as a browser reports it (lower case, ASCII, a Unicode name in its punycode form, an IP address in its usual form), without a scheme, user, port, path, query, leading `*.` or `.`, or trailing dot. So `https://Bücher.de/page`, `*.bücher.de`, `.bücher.de` and `xn--bcher-kva.de.` are all `xn--bcher-kva.de`. Null when no host is left, or what is left is not a host name; that includes a `*` anywhere else, which no host a browser reports holds.
 */
export function normalizeSite(input: string): string | null {
  let site = input.replace(SURROUNDING_WHITE_SPACE, '').replace(SCHEME, '');
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
  const type = input.replace(SURROUNDING_WHITE_SPACE, '').replace(LEADING_WILDCARDS, '');
  return type === '' ? null : type.toLowerCase();
}

/** `list` with each entry kept the way `normalizeSite` keeps it, in order, without the ones that name no host or one already there. */
function normalizedSites(list: readonly string[]): string[] {
  const sites = new Set<string>();
  for (const entry of list) {
    const site = normalizeSite(entry);
    if (site !== null) sites.add(site);
  }
  return [...sites];
}

/**
 * `settings` with `patch` applied the way the app applies it (`ExtensionSettings::apply`): the whole list of ignored sites first, then additions, then removals, each site taken the way `normalizeSite` keeps it; a field that is null or left out keeps its value.
 */
export function applyExtensionSettingsPatch(
  settings: ExtensionSettings,
  patch: ExtensionSettingsPatch,
): ExtensionSettings {
  // The list is kept as it is unless the patch replaces it; the set is only for telling what it holds.
  const sites = patch.ignored_sites
    ? normalizedSites(patch.ignored_sites)
    : [...settings.ignored_sites];
  const kept = new Set(sites);
  for (const site of normalizedSites(patch.add_ignored_sites ?? [])) {
    if (!kept.has(site)) sites.push(site);
    kept.add(site);
  }
  const removed = new Set(normalizedSites(patch.remove_ignored_sites ?? []));
  return {
    capture_enabled: patch.capture_enabled ?? settings.capture_enabled,
    ask_before_download: patch.ask_before_download ?? settings.ask_before_download,
    show_media_button: patch.show_media_button ?? settings.show_media_button,
    ignored_sites: sites.filter((site) => !removed.has(site)),
    capture_extensions: patch.capture_extensions ?? settings.capture_extensions,
    min_capture_size: patch.min_capture_size ?? settings.min_capture_size,
    default_queue: patch.default_queue ?? settings.default_queue,
  };
}
