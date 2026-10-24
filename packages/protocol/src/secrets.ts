// What a secret is, for the extension's redaction: ports of `is_secret_name`, `names_a_password`, `looks_like_a_credential` and `is_service_key` in crates/protocol/src/secrets.rs, which the log's redaction calls, tested against the cases both languages load from `crates/test-support/data/redaction.json` (`./testing/redaction`), so both redactions agree on what a secret is (D40.10).
import {
  CREDENTIAL_MIN_CHARS,
  KEY_MIN_REST,
  KEY_PREFIXES,
  PASSWORD_WORDS,
  SECRET_ENDINGS,
  SECRET_WORDS,
} from './generated/constants';

/** Where a name splits into segments: `-`, `_` and `.`, and a camelCase step. */
const SEGMENT_BREAK = /[-_.]+|(?<=[a-z\d])(?=[A-Z])/;
const ASCII_CAPITAL = /[A-Z]/g;
const CAPITAL = /[A-Z]/;
const DIGIT_OR_CAPITAL = /[\dA-Z]/;
const CREDENTIAL_SIGN = /[\d+/=]/;

/** `text` in lowercase as Rust's `to_ascii_lowercase` has it: only `A` to `Z` change. */
const asciiLowercase = (text: string): string =>
  text.replaceAll(ASCII_CAPITAL, (capital) => capital.toLowerCase());

/** The segments of `name`, in ASCII lowercase. */
const segmentsOf = (name: string): string[] => name.split(SEGMENT_BREAK).map(asciiLowercase);

/** How many characters `text` has (code points, as Rust's `chars().count()`). */
const charCount = (text: string): number => Array.from(text).length;

/**
 * Whether the value named `name` is a secret: one of the name's segments (split at `-`, `_`, `.` and camelCase steps) is one of `SECRET_WORDS` or ends with one of `SECRET_ENDINGS`.
 */
export function isSecretName(name: string): boolean {
  return segmentsOf(name).some(
    (segment) =>
      segment !== '' &&
      (SECRET_WORDS.includes(segment) || SECRET_ENDINGS.some((ending) => segment.endsWith(ending))),
  );
}

/** Whether the value named `name` is a password, which after a `:` may be several words. */
export function namesAPassword(name: string): boolean {
  return segmentsOf(name).some((segment) => PASSWORD_WORDS.includes(segment));
}

/**
 * Whether a word after an authorization scheme that is also a common word (`basic`) looks like a credential rather than prose: long enough, with a digit, a `+`, `/` or `=`, or a capital after the first character.
 */
export function looksLikeACredential(word: string): boolean {
  const chars = Array.from(word);
  return (
    chars.length >= CREDENTIAL_MIN_CHARS &&
    chars.some((char, i) => CREDENTIAL_SIGN.test(char) || (i > 0 && CAPITAL.test(char)))
  );
}

/** Whether `word` starts with a service's key prefix and goes on long enough, with a digit or a capital. */
export function isServiceKey(word: string): boolean {
  return KEY_PREFIXES.some((prefix) => {
    if (!word.startsWith(prefix)) return false;
    const rest = word.slice(prefix.length);
    return charCount(rest) >= KEY_MIN_REST && DIGIT_OR_CAPITAL.test(rest);
  });
}
