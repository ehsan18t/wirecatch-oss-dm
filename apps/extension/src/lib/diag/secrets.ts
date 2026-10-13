/**
 * Hiding secrets in a text by the name they are given: a header (`Cookie: a=1; b=2`), a pair (`access_token=x`, `password => 'x'`), a JSON key or an object field (`"password": "x"`), a tag (`<password>x</password>`), a header list entry as the browser writes it (`{"name": "Cookie", "value": "x"}`, `["Cookie", "x"]`), and any of those written as a string inside another string (`\"Cookie\":\"x\"`); and the credential after an authorization scheme.
 *
 * What a secret's name is, which headers carry a sign-in and which words are schemes come from `@wirecatch/protocol`, which the log's redaction reads too, and both are tested against the samples in `crates/test-support/data/redaction.json` (D40.10). A name is a secret's when one of its segments, split at `-`, `_`, `.` and camelCase steps, is a secret word or ends in one: `x-amz-security-token`, `apiKey` and `accesstoken` are, `design` and `author` are not. Its value is read by its shape, so a quoted value is hidden to its closing quote whatever it holds, a list or an object to its closing bracket, a `Cookie` or `Authorization` header (and `password:`) to the end of its field, continued lines included, any other value after `:` to the end of its field on its line, a scheme word with the credential after it, and anything else to the next separator.
 */
import {
  AUTH_SCHEMES,
  AUTHORIZATION_HEADERS,
  COOKIE_HEADERS,
  isSecretName,
  looksLikeACredential,
  namesAPassword,
} from '@wirecatch/protocol';

/** What stands in place of a hidden value. */
export const HIDDEN = '[redacted]';

/**
 * Headers whose value runs on past spaces and quotes (`Cookie: a=1; b="2"`), to the end of the field: its line, and the lines after it that do not start a field of their own.
 */
const LINE_HEADERS = new Set([...COOKIE_HEADERS, ...AUTHORIZATION_HEADERS]);
const SCHEME_WORDS = new Set(AUTH_SCHEMES);
/** How `Authorization` spells the one scheme after which any word is a credential. */
const BEARER = 'Bearer';

/** A name, bounded so that a long run of name characters costs a fixed amount where it is tried. */
const NAME = String.raw`\w[\w.-]{0,39}`;
/** A quote, as written or escaped inside another string once or more (`"`, `\"`, `\\\"`). */
const QUOTE = String.raw`\\*["']`;
const MAYBE_QUOTE = `(?:${QUOTE})?`;
/** A quoted string, escapes included, on one line. */
const QUOTED = String.raw`"(?:[^"\\\n]|\\.)*"|'(?:[^'\\\n]|\\.)*'`;

// Each pattern below ends where the value starts, and names the secret's name `name` and the quote around it, if any, `q`.
/** `name: value`, `name=value` and `"name": value`: a header, a pair, a JSON key or a field. */
const NAMED = new RegExp(
  String.raw`(?<q>${QUOTE})?\b(?<name>${NAME})\k<q>\s*(?<separator>=>|[:=])\s*`,
  'g',
);
/** `<name>value</name>`: the value runs to the next tag. */
const TAGGED = new RegExp(String.raw`<(?<name>${NAME})>[^<\r\n]+`, 'g');
/** A header list entry, `["Cookie", value]`. */
const LISTED = new RegExp(String.raw`\[\s*(?<q>${QUOTE})(?<name>${NAME})\k<q>\s*,\s*`, 'g');
/** A header object, name first: `{"name": "Cookie", "value": value}`, or a form, `name=Cookie&value=…`. */
const NAME_FIRST = new RegExp(
  String.raw`\bname${MAYBE_QUOTE}\s*[:=]\s*(?<q>${QUOTE})?(?<name>${NAME})\k<q>\s*[,&;]\s*${MAYBE_QUOTE}value${MAYBE_QUOTE}\s*[:=]\s*`,
  'g',
);
/** A header object, value first: `{"value": "...", "name": "Cookie"}`, matched whole. */
const VALUE_FIRST = new RegExp(
  String.raw`(?<before>\bvalue${MAYBE_QUOTE}\s*[:=]\s*)(?<value>${QUOTED})(?<after>\s*,\s*${MAYBE_QUOTE}name${MAYBE_QUOTE}\s*[:=]\s*${QUOTE}(?<name>${NAME}))`,
  'g',
);
/** The quote that opens a value, escaped or not. */
const OPENING_QUOTE = new RegExp(QUOTE, 'y');
/** An unquoted value: up to the next space, separator, quote or closing bracket. */
const WORD = /[^\s&;,"'<>}\]]+/y;
/** The name of the next pair after an `&`: `x=` in `access_token=abc&x=1`. */
const PAIR_NAME = new RegExp(`${NAME}=`, 'y');
const LINE_END = /[\r\n]/g;
const LINE_BREAK = /\r\n|\r|\n/y;
/** What starts a field of its own at the start of a line: `User-Agent:`. */
const NEW_FIELD = /[\w-]+[ \t]*:/y;
/** What separates a scheme word from its credential: blanks and at most one line break, as a header folds it. */
const SCHEME_GAP = String.raw`(?=\s)[ \t]*(?:\r\n|[\r\n])?[ \t]*`;
/** An authorization scheme in any case, and the word after it. */
const SCHEME = new RegExp(
  String.raw`\b(?<scheme>${AUTH_SCHEMES.join('|')})${SCHEME_GAP}(?<credential>(?!\[)[\w\-.~+/]+=*)`,
  'gi',
);
/** The credential after a scheme word that starts a secret's value, already hidden or not. */
const SCHEME_CREDENTIAL = new RegExp(String.raw`${SCHEME_GAP}(?:\[redacted\]|[\w\-.~+/]+=*)`, 'y');

const isSchemeWord = (word: string): boolean => SCHEME_WORDS.has(word.toLowerCase());

/** Where the line holding `from` ends. */
function lineEnd(text: string, from: number): number {
  LINE_END.lastIndex = from;
  return LINE_END.exec(text)?.index ?? text.length;
}

/**
 * Where the field whose value starts at `from` ends: the end of its line, or of a later line when the ones after it continue it (folded, or wrapped by whatever wrote the message) rather than start a field of their own or leave a blank line.
 */
function fieldEnd(text: string, from: number): number {
  let end = lineEnd(text, from);
  while (end < text.length) {
    LINE_BREAK.lastIndex = end;
    const next = end + (LINE_BREAK.exec(text)?.[0].length ?? 1);
    NEW_FIELD.lastIndex = next;
    const char = text.charAt(next);
    if (next >= text.length || char === '\r' || char === '\n' || NEW_FIELD.test(text)) break;
    end = lineEnd(text, next);
  }
  return end;
}

/**
 * Where the string opened by `quote` (a quote after as many backslashes as it was escaped) closes: at the same quote after exactly as many backslashes. -1 when it never does.
 */
function closingQuote(text: string, from: number, quote: string): number {
  const mark = quote.slice(-1);
  const escapes = quote.length - 1;
  for (let at = text.indexOf(mark, from); at !== -1; at = text.indexOf(mark, at + 1)) {
    let run = 0;
    while (text[at - run - 1] === '\\') run += 1;
    if (run === escapes) return at - escapes;
  }
  return -1;
}

/** Where the list or object opened at `start` closes, skipping strings; -1 when it never does. */
function closingBracket(text: string, start: number): number {
  let depth = 0;
  for (let at = start; at < text.length; at += 1) {
    const char = text.charAt(at);
    if (char === '"' || char === "'") {
      at = closingQuote(text, at + 1, char);
      if (at === -1) return -1;
    } else if (char === '[' || char === '{') depth += 1;
    else if (char === ']' || char === '}') {
      depth -= 1;
      if (depth === 0) return at;
    }
  }
  return -1;
}

/**
 * How far a secret's unquoted value runs: over the lines that continue it (a header line, a password after `:`), to the end of its field (any other secret after `:`), or to the end of its word.
 */
type Extent = 'lines' | 'field' | 'word';

interface Hidden {
  /** Where the value ended. */
  end: number;
  /** What stands in its place. */
  text: string;
}

/** The value starting at `start`, hidden whatever its shape; null when there is none to hide. */
function hideValue(text: string, start: number, extent: Extent): Hidden | null {
  if (text.startsWith(HIDDEN, start)) return null;
  if (extent === 'lines') return { end: fieldEnd(text, start), text: HIDDEN };
  OPENING_QUOTE.lastIndex = start;
  const quote = OPENING_QUOTE.exec(text)?.[0];
  if (quote !== undefined) {
    const close = closingQuote(text, start + quote.length, quote);
    if (close === -1) return { end: text.length, text: `${quote}${HIDDEN}` };
    return { end: close + quote.length, text: `${quote}${HIDDEN}${quote}` };
  }
  const char = text.charAt(start);
  if (char === '[' || char === '{') {
    const close = closingBracket(text, start);
    return { end: close === -1 ? text.length : close + 1, text: HIDDEN };
  }
  if (extent === 'field') {
    const end = valueFieldEnd(text, start);
    return { end: start + ownLength(text.slice(start, end)), text: HIDDEN };
  }
  return hideWord(text, start);
}

/**
 * Where a value after `name:` ends on its line, as the log reads it: at the end of the line, at ` / ` (how the log joins a program's last lines), or at a space before another `name=` that does not follow a `;` or `,`.
 */
function valueFieldEnd(text: string, start: number): number {
  const end = lineEnd(text, start);
  for (let at = text.indexOf(' ', start); at !== -1 && at < end; at = text.indexOf(' ', at + 1)) {
    if (text.startsWith('/ ', at + 1)) return at;
    const afterPair = at > start && (text.charAt(at - 1) === ';' || text.charAt(at - 1) === ',');
    PAIR_NAME.lastIndex = at + 1;
    if (!afterPair && PAIR_NAME.test(text)) return at;
  }
  return end;
}

/** The unquoted value starting at `start`, hidden with the credential after it when it is a scheme word; null when there is none. */
function hideWord(text: string, start: number): Hidden | null {
  const end = wordEnd(text, start);
  if (end === null) return null;
  if (!isSchemeWord(text.slice(start, end))) return { end, text: HIDDEN };
  // `X-Auth-Token: Bearer abc`: the scheme says the word after it is the secret.
  SCHEME_CREDENTIAL.lastIndex = end;
  const credential = SCHEME_CREDENTIAL.exec(text)?.[0] ?? '';
  return { end: end + credential.length, text: HIDDEN };
}

/**
 * Where an unquoted value from `start` ends: at the next separator, except an `&` no `name=` follows, which belongs to the value (`password=a&b`), as in the log; null when there is none.
 */
function wordEnd(text: string, start: number): number | null {
  let end = start;
  for (;;) {
    WORD.lastIndex = end;
    const word = WORD.exec(text)?.[0];
    if (word === undefined) return end === start ? null : end;
    const own = ownLength(word);
    end += own;
    if (own < word.length || text.charAt(end) !== '&') return end;
    PAIR_NAME.lastIndex = end + 1;
    if (PAIR_NAME.test(text)) return end;
    end += 1;
  }
}

/** How much of an unquoted value is its own: up to a `)` it did not open, which closes the text around it. */
function ownLength(word: string): number {
  let depth = 0;
  for (let at = 0; at < word.length; at += 1) {
    const char = word.charAt(at);
    if (char === '(') depth += 1;
    else if (char === ')') {
      if (depth === 0) return at;
      depth -= 1;
    }
  }
  return word.length;
}

/** How far an unquoted name's value runs: over continued lines for a header line or `password: words`, to the end of the field after any other `name:`, else one word. */
function extentOf(name: string, match: RegExpExecArray): Extent {
  const colon = match.groups?.separator === ':';
  if (LINE_HEADERS.has(name.toLowerCase()) || (colon && namesAPassword(name))) return 'lines';
  return colon ? 'field' : 'word';
}

/**
 * Hides the value after each match of `pattern` whose name is a secret's. A name that is not leaves its value to be read again, since the value may itself hold a secret pair.
 */
function hideAfter(text: string, pattern: RegExp, headers: boolean): string {
  let kept = '';
  let from = 0;
  pattern.lastIndex = 0;
  for (let match = pattern.exec(text); match !== null; match = pattern.exec(text)) {
    const name = match.groups?.name ?? '';
    if (!isSecretName(name)) continue;
    const start = match.index + match[0].length;
    const extent = headers && match.groups?.q === undefined ? extentOf(name, match) : 'word';
    const hidden = hideValue(text, start, extent);
    if (hidden === null) continue;
    kept += `${text.slice(from, start)}${hidden.text}`;
    from = hidden.end;
    pattern.lastIndex = hidden.end;
  }
  return `${kept}${text.slice(from)}`;
}

/** `{"value": "...", "name": "Cookie"}` with the value hidden when the name is a secret's. */
function hideValueFirst(text: string): string {
  let kept = '';
  let from = 0;
  for (const match of text.matchAll(VALUE_FIRST)) {
    const { before = '', value = '', after = '', name = '' } = match.groups ?? {};
    if (!isSecretName(name)) continue;
    const quote = value.charAt(0);
    kept += `${text.slice(from, match.index)}${before}${quote}${HIDDEN}${quote}${after}`;
    from = match.index + match[0].length;
  }
  return `${kept}${text.slice(from)}`;
}

/**
 * `text` with the credential after every authorization scheme hidden: any word after `Bearer` as a header spells it, and after the others only a word that looks like one, since `basic` is also a common word.
 */
function hideSchemeCredentials(text: string): string {
  return text.replace(SCHEME, (whole: string, scheme: string, credential: string) => {
    const hidden = scheme === BEARER || looksLikeACredential(credential);
    return hidden ? `${whole.slice(0, whole.length - credential.length)}${HIDDEN}` : whole;
  });
}

/** `<password>x</password>` with the value hidden when the tag is a secret's name. */
const hideTagged = (text: string): string =>
  text.replace(TAGGED, (whole: string, name: string) =>
    isSecretName(name) ? `<${name}>${HIDDEN}` : whole,
  );

/** `text` with every credential after an authorization scheme hidden, and the value of every secret it names. */
export function hideSecrets(text: string): string {
  // Schemes first, as the log does, so a secret's value that starts with one takes the credential too.
  let clean = hideSchemeCredentials(text);
  clean = hideValueFirst(clean);
  clean = hideAfter(clean, NAME_FIRST, false);
  clean = hideAfter(clean, LISTED, false);
  clean = hideAfter(clean, NAMED, true);
  return hideTagged(clean);
}
