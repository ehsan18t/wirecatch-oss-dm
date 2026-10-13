/**
 * What may go into a diagnostics entry. Error messages can carry the address a request went to,
 * a header the browser echoed back, or a piece of the page, and diagnostics are meant to be
 * copied into a support request, so every text is cut down to what explains a failure: the site
 * of each address, with or without a scheme, never its path, query, fragment or user name and
 * password; no file path, which names the user, and no e-mail address; and no cookie,
 * authorization header, token or other secret, found by the name it is given (`secrets.ts`) and
 * by its shape (a JWT, a long opaque run) (docs/ENGINEERING.md, sections 6 and 11). Percent-encoded
 * text is decoded first, so an encoded address or secret is read as what it encodes.
 */
import { isServiceKey } from '@wirecatch/protocol';
import { HIDDEN, hideSecrets } from './secrets';

/** Longest text one entry keeps; past it, the text is cut and ends with an ellipsis. */
export const MAX_TEXT = 300;
/** Longest input read at all, so a page-sized message costs no more than a few entries. */
const MAX_INPUT = 4 * MAX_TEXT;

const ELLIPSIS = '…';

/** Absolute file paths, which name the user: `C:\Users\…`, `C:/Users/…`, `\\server\share`, `~/…` and `/a/b`. */
const WINDOWS_PATH = /\b[a-z]:[\\/][^\s"'<>]*/gi;
/** A share is two backslashes, a host and a backslash; two backslashes alone are an escape (`\\\"`). */
const UNC_PATH = /\\\\[\w.$-]+\\[^\s"'<>]*/g;
const UNIX_PATH = /(^|[\s("'=])(?:~\/|\/(?!\/)[^\s/"'<>]+\/)[^\s"'<>]*/g;
/** Addresses that are their own contents: a `data:` URL is the file, a `javascript:` URL is code. */
const INLINE_URL = /\b(data|javascript):[^\s"'<>]+/gi;
/** Any address with an authority, `scheme://...`, up to the next space or quote. */
const URL_WITH_AUTHORITY = /\b[a-z][a-z\d+.-]*:\/\/[^\s"'<>]+/gi;
/** An address without a scheme: protocol-relative `//host/...`, or a `www.` host. */
const RELATIVE_URL = /(^|[\s("'=])\/\/([^\s/?#"'<>]+)([^\s"'<>]*)/g;
const WWW_URL = /\b(www\.[^\s/?#"'<>]+)([^\s"'<>]*)/gi;
/**
 * A bare host followed by a path, `example.com/download/x.mp4`: labels and a top-level label of
 * letters, `localhost` or an IPv4 address, then an optional port.
 */
const BARE_HOST =
  /(^|[\s("'=])((?:(?:[a-z\d](?:[a-z\d-]{0,61}[a-z\d])?\.)+[a-z]{2,63}|localhost|\d{1,3}(?:\.\d{1,3}){3})(?::\d{1,5})?)(\/[^\s"'<>]*)/gi;
/**
 * Any other scheme (`mailto:`, `magnet:`, `about:`, `blob:`), kept by name only. Not a host's
 * port, a value already hidden (`Cookie:[redacted]`), nor a code after `::` (`net::ERR_ABORTED`).
 */
const OTHER_SCHEME =
  /(?<![\w./@-])([a-z][a-z\d+.-]+):(?!\/\/|:|\[redacted\]|\d+(?:[/?#\s.,;)]|$))[^\s"'<>]+/gi;
/** What usually closes a sentence or a bracket around an address, not part of it. */
const TRAILING_PUNCTUATION = /[).,;:!?\]}]+$/;
/** `name:password@` written without a scheme. */
const BARE_CREDENTIALS = /[^\s/@:]+:[^\s/@]+@/g;
const EMAIL = /[\w.+-]+@[\w-]+(?:\.[\w-]+)+/g;
/** A query or fragment still attached to a word (`page.html?q=…`). */
const QUERY = /([^\s?#])([?#])[^\s"'<>]+/g;
/** Token shapes, whatever names them: a JWT, or a long opaque run such as a key or a hash. */
const JWT = /\beyJ[\w-]+\.[\w-]+\.[\w-]*/g;
/** A run that may be a key: base64 included (`+`, `/`, `=`), since paths are gone by then. */
const OPAQUE = /\b[\w+/-]{12,}={0,2}/g;
/** A part of a run, between `-`, `_`, `.` and `/`, this long is opaque however it is written... */
const OPAQUE_PART = 32;
/**
 * ...and one this long is when it mixes letters and digits, as a key does. Shorter mixed parts
 * are kept: media segments, build chunks and request ids are what places a failure.
 */
const MIXED_PART = 20;
/** Base64 with its padding is opaque from this length. */
const PADDED_PART = 12;
/** Runs of percent-encoded bytes. */
const PERCENT_RUN = /(?:%[\da-f]{2})+/gi;
/** One encoded byte of the ASCII range, which decodes on its own. */
const PERCENT_ASCII = /%([0-7][\da-f])/gi;
/** How many times encoded text is decoded: an encoded address inside an encoded parameter is two. */
const DECODE_ROUNDS = 3;
/** Line breaks and other control characters, which could forge an entry of their own. */
const CONTROL = /\p{Cc}+/gu;

/** Whether the bracket at `at` closes one opened earlier in `text` (`?sig=[redacted]`, `[::1]`). */
function closesWithin(text: string, at: number): boolean {
  const close = text.charAt(at);
  const open = close === ']' ? '[' : close === ')' ? '(' : null;
  if (open === null) return false;
  const before = text.slice(0, at);
  return before.split(open).length > before.split(close).length;
}

/** `address` split into itself and the punctuation that closes the sentence around it. */
function split(address: string): [string, string] {
  const trailing = TRAILING_PUNCTUATION.exec(address)?.[0] ?? '';
  let end = address.length - trailing.length;
  while (end < address.length && closesWithin(address, end)) end += 1;
  return [address.slice(0, end), address.slice(end)];
}

/** The site of an address, with an ellipsis when it had more after it. */
function siteOf(address: string): string {
  const [bare, trailing] = split(address);
  let url: URL;
  try {
    url = new URL(bare);
  } catch {
    // Not an address after all; nothing of it is kept, since it may still hold a secret.
    return `[address]${trailing}`;
  }
  const site = url.host === '' ? `${url.protocol}//` : `${url.protocol}//${url.host}`;
  const more = url.pathname.replace(/^\/$/, '') !== '' || url.search !== '' || url.hash !== '';
  return `${site}${more ? `/${ELLIPSIS}` : ''}${trailing}`;
}

/** A host written without a scheme, with an ellipsis when a path, query or fragment followed. */
function hostWith(host: string, rest: string): string {
  const [more, trailing] = split(rest);
  return `${host}${more === '' ? '' : `/${ELLIPSIS}`}${trailing}`;
}

/** One run of encoded bytes, decoded; when it is not UTF-8, only its ASCII bytes are. */
function decodeRun(run: string): string {
  try {
    return decodeURIComponent(run);
  } catch {
    // Not UTF-8 once decoded: what reads as ASCII is still decoded, so nothing hides behind it.
    return run.replace(PERCENT_ASCII, (_m, hex: string) => String.fromCharCode(parseInt(hex, 16)));
  }
}

/** `text` with its percent-encoded parts decoded, as many times as they were encoded, within reason. */
function decodePercent(text: string): string {
  let decoded = text;
  for (let round = 0; round < DECODE_ROUNDS; round += 1) {
    const next = decoded.replace(PERCENT_RUN, decodeRun);
    if (next === decoded) break;
    decoded = next;
  }
  return decoded;
}

const mixesLetterAndDigit = (text: string): boolean => /\d/.test(text) && /[a-z]/i.test(text);
const mixesCasesAndDigit = (text: string): boolean =>
  /\d/.test(text) && /[a-z]/.test(text) && /[A-Z]/.test(text);

/**
 * Whether a run reads as a key or a hash rather than a name: a service's key (`isServiceKey`, which the log applies too); base64 with its padding; base64 cut by `/` that mixes cases and digits (an AWS secret key); or a part that is very long, or long and mixes letters and digits.
 */
function isOpaque(run: string): boolean {
  if (isServiceKey(run)) return true;
  const bare = run.replace(/=+$/, '');
  if (bare !== run && bare.length >= PADDED_PART && mixesLetterAndDigit(bare)) return true;
  if (bare.includes('/') && bare.length >= MIXED_PART && mixesCasesAndDigit(bare)) return true;
  return bare
    .split(/[-_./]/)
    .some(
      (part) =>
        part.length >= OPAQUE_PART || (part.length >= MIXED_PART && mixesLetterAndDigit(part)),
    );
}

/** Reduces every address to its site and every path, e-mail address and query to a placeholder. */
function reduceAddresses(text: string): string {
  return text
    .replace(WINDOWS_PATH, '[path]')
    .replace(UNC_PATH, '[path]')
    .replace(UNIX_PATH, (_m, before: string) => `${before}[path]`)
    .replace(INLINE_URL, (_m, scheme: string) => `${scheme}:${ELLIPSIS}`)
    .replace(URL_WITH_AUTHORITY, siteOf)
    .replace(
      RELATIVE_URL,
      (_m, before: string, host: string, rest: string) => `${before}//${hostWith(host, rest)}`,
    )
    .replace(WWW_URL, (_m, host: string, rest: string) => hostWith(host, rest))
    .replace(
      BARE_HOST,
      (_m, before: string, host: string, rest: string) => `${before}${hostWith(host, rest)}`,
    )
    .replace(BARE_CREDENTIALS, `${HIDDEN}@`)
    .replace(EMAIL, '[email]')
    .replace(OTHER_SCHEME, (_m, scheme: string) => `${scheme}:${ELLIPSIS}`)
    .replace(QUERY, (_m, before: string, mark: string) => `${before}${mark}${ELLIPSIS}`);
}

/** `text` with every address reduced to its site and every secret hidden, on one line and at most `MAX_TEXT` long. */
export function redact(text: string): string {
  // Secrets before any other rewriting, which could change the quotes and escapes they are found
  // by; a placeholder left inside an address is reduced with it.
  const clean = reduceAddresses(hideSecrets(decodePercent(text.slice(0, MAX_INPUT))))
    .replace(JWT, '[token]')
    .replace(OPAQUE, (run) => (isOpaque(run) ? '[token]' : run))
    .replace(CONTROL, ' ')
    .trim();
  return clean.length > MAX_TEXT ? `${clean.slice(0, MAX_TEXT - 1)}${ELLIPSIS}` : clean;
}

/** A string field of an object, or null. */
function stringField(value: unknown, key: string): string | null {
  if (typeof value !== 'object' || value === null || !(key in value)) return null;
  const field: unknown = Reflect.get(value, key);
  return typeof field === 'string' ? field : null;
}

/** What kind of failure an error says it is: a request error's `kind`, and its protocol error's `code`. */
function kindsOf(error: Error): string {
  const kinds = [
    stringField(error, 'kind'),
    stringField(Reflect.get(error, 'protocolError'), 'code'),
  ];
  const known = kinds.filter((kind) => kind !== null);
  return known.length === 0 ? '' : ` [${known.join(', ')}]`;
}

/** An error's name, kind and message, without what it wraps. */
function ownMessage(error: Error): string {
  const head = `${error.name}${kindsOf(error)}`;
  return error.message === '' ? head : `${head}: ${error.message}`;
}

/** What an error wraps, one level deep: its `cause`, or the errors of an `AggregateError`. */
function wrapped(error: Error): string {
  if (error instanceof AggregateError) {
    const errors: unknown[] = Array.isArray(error.errors) ? error.errors : [];
    const first: unknown = errors[0];
    const firstText = first instanceof Error ? `, first: ${ownMessage(first)}` : '';
    return ` (${String(errors.length)} errors${firstText})`;
  }
  const cause: unknown = error.cause;
  if (cause === undefined) return '';
  const text =
    cause instanceof Error ? ownMessage(cause) : (stringField(cause, 'message') ?? typeof cause);
  return ` (cause: ${text})`;
}

/** The message of an `Error`, or of an error-shaped object such as `runtime.lastError`. */
function messageOf(error: object): string | null {
  if (error instanceof Error) return `${ownMessage(error)}${wrapped(error)}`;
  return stringField(error, 'message');
}

/**
 * A redacted one-line description of anything thrown. Never throws itself, whatever it is given:
 * a getter that throws, a proxy, or an object with no prototype. Objects other than errors are
 * named by their kind only, since their fields may be anything, page contents included.
 */
export function describeError(error: unknown): string {
  try {
    if (typeof error === 'string') return redact(error);
    if (typeof error === 'object' && error !== null) {
      return redact(messageOf(error) ?? Object.prototype.toString.call(error));
    }
    if (typeof error === 'symbol' || typeof error === 'function') return typeof error;
    return String(error);
  } catch {
    // The error's own fields threw while being read; what it was is lost, but not that it happened.
    return 'an error that could not be read';
  }
}
