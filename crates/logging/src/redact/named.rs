//! The values of names that mark a secret: `password=..`, `Cookie: ..`, `"token": ".."`, `--password ..`, in all the ways they are written. What a secret's name is, and which headers carry a sign-in, is `wirecatch_protocol::secrets`, which the extension's redaction reads too (D40.10).

use wirecatch_protocol::secrets::{
    AUTHORIZATION_HEADERS, COOKIE_HEADERS, is_secret_name, names_a_password,
};

use super::scan::{scan_until, steps};
use super::{HIDDEN, credential_end, is_scheme_word, past_scheme_gap};

/// Name segments that make a command-line flag's value no secret: `--cookies` takes a file and `--cookies-from-browser` a browser's name, and yt-dlp names both in the advice it prints.
const NOT_SECRET_AS_FLAGS: [&str; 2] = ["cookie", "cookies"];

/// Fields of `url::Url`'s `Debug` that carry what the text of its link would: its user name, query and fragment. A secret only when written as a `Debug` field holding a string (`query: Some("..")`), since the words are common.
const LINK_FIELD_NAMES: [&str; 3] = ["username", "query", "fragment"];

/// Command-line flags, matched without their dashes and with `-` removed, whose value is a secret besides those with a secret name: yt-dlp's `-p` (`--password`) and `--add-header`, ffmpeg's `-headers`.
const SECRET_FLAGS: [&str; 4] = ["p", "addheader", "addheaders", "headers"];

/// The key of a header object's value, `{"name": "Cookie", "value": ".."}`.
const VALUE_KEY: &[u8] = b"value";

/// The key of a header object's name.
const NAME_KEY: &[u8] = b"name";

/// How many `Debug` wrappers around a value are looked through: `Some(Secret("..."))`.
const MAX_WRAPPERS: usize = 3;

/// Whether a byte can be part of a name such as `Set-Cookie`, `access_token`, `--password` or `auth.token`.
fn is_name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.')
}

/// How the value of a secret name is written, which decides where it ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// A word, or what is quoted (`password=abc`).
    Value,
    /// A password, whose value after `:` may be several words and go on over the lines that continue it.
    Password,
    /// The rest of the field, as a cookie header's value (`Cookie: a=1; b=2`), and the lines that continue it.
    WholeField,
    /// The rest of the line (`Authorization: Digest username="ada", response=".."`), and the lines that continue it.
    WholeLine,
    /// Only a string in a `Debug` field (`query: Some("..")`).
    LinkField,
    /// A command-line flag's value, which may follow a space (`--password abc`).
    Flag,
}

/// Whether `bare` (a name without its leading dashes) is `flag`, compared without `-`, `_` and `.` and in any case.
fn is_flag(bare: &str, flag: &str) -> bool {
    bare.bytes()
        .filter(|b| !matches!(b, b'-' | b'_' | b'.'))
        .map(|b| b.to_ascii_lowercase())
        .eq(flag.bytes())
}

/// What a name says about its value: `None` when it is no secret.
fn kind_of(name: &[u8]) -> Option<Kind> {
    // A name is made of ASCII bytes only (`is_name_byte`).
    let name = std::str::from_utf8(name).ok()?;
    let bare = name.trim_start_matches('-');
    if bare.len() < name.len() {
        let secret = bare
            .split(['-', '_', '.'])
            .filter(|part| {
                !NOT_SECRET_AS_FLAGS
                    .iter()
                    .any(|word| part.eq_ignore_ascii_case(word))
            })
            .any(is_secret_name);
        return (secret || SECRET_FLAGS.iter().any(|flag| is_flag(bare, flag)))
            .then_some(Kind::Flag);
    }
    let is_one_of = |names: &[&str]| names.iter().any(|header| bare.eq_ignore_ascii_case(header));
    if is_one_of(AUTHORIZATION_HEADERS) {
        Some(Kind::WholeLine)
    } else if is_one_of(COOKIE_HEADERS) {
        Some(Kind::WholeField)
    } else if names_a_password(bare) {
        Some(Kind::Password)
    } else if is_secret_name(bare) {
        Some(Kind::Value)
    } else if is_one_of(&LINK_FIELD_NAMES) {
        Some(Kind::LinkField)
    } else {
        None
    }
}

/// Whether a value named `name` (a log field's name) is a secret: what [`text_for_log`](super::text_for_log) hides after `name=`.
#[cfg(feature = "setup")]
pub(crate) fn names_a_secret(name: &str) -> bool {
    kind_of(name.as_bytes()).is_some_and(|kind| kind != Kind::LinkField)
}

/// `text` with the value of every secret name (see [`kind_of`]) replaced by [`HIDDEN`], and of every header object's `value` whose `name` is a secret's (`{"value": "..", "name": "Cookie"}`).
pub(super) fn hide_named_secrets(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        let before = i.checked_sub(1).and_then(|before| bytes.get(before));
        if !is_name_byte(b) || before.is_some_and(|before| is_name_byte(*before)) {
            steps::add(1);
            i += 1;
            continue;
        }
        let name_end = scan_until(bytes, i, |b| !is_name_byte(b));
        let name = bytes.get(i..name_end).unwrap_or_default();
        // A segment of a module path (`wirecatch_core::cookies: ...`) names no value.
        let in_path = i >= 2 && bytes.get(i - 2..i) == Some(b"::");
        let value = if in_path {
            None
        } else if let Some(kind) = kind_of(name) {
            secret_value(bytes, i, name_end, kind)
        } else if name.eq_ignore_ascii_case(VALUE_KEY) {
            value_named_after(bytes, name_end)
        } else {
            None
        };
        match value {
            Some((start, end)) => {
                out.push_str(&text[copied..start]);
                out.push_str(HIDDEN);
                copied = end;
                i = end;
            }
            None => i = name_end,
        }
    }
    out.push_str(&text[copied..]);
    out
}

/// A quote that opens or closes a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Quote {
    /// `"` or `'`.
    Plain(u8),
    /// `\"` or `\'`, as `Debug` writes a quote inside a string, after this many backslashes (`\\\"` in the `Debug` of a `Debug`).
    Escaped(u8, usize),
    /// `%22`, a URL-encoded `"`.
    Encoded,
}

impl Quote {
    /// The quote that starts at `at`, if any.
    fn at(bytes: &[u8], at: usize) -> Option<Self> {
        match *bytes.get(at)? {
            quote @ (b'"' | b'\'') => Some(Self::Plain(quote)),
            b'\\' => {
                let slashes_end = scan_until(bytes, at, |b| b != b'\\');
                match *bytes.get(slashes_end)? {
                    quote @ (b'"' | b'\'') => Some(Self::Escaped(quote, slashes_end - at)),
                    _ => None,
                }
            }
            b'%' if bytes.get(at + 1..at + 3) == Some(b"22") => Some(Self::Encoded),
            _ => None,
        }
    }

    fn len(self) -> usize {
        match self {
            Self::Plain(_) => 1,
            Self::Escaped(_, slashes) => slashes + 1,
            Self::Encoded => 3,
        }
    }

    /// Where the value this quote opened, from `inside`, ends: at the closing quote (an escaped one inside it does not close it), or else at the end of the line.
    fn close(self, bytes: &[u8], inside: usize) -> usize {
        let mut at = inside;
        while let Some(&b) = bytes.get(at) {
            steps::add(1);
            if b == b'\n' || b == b'\r' {
                return at;
            }
            match self {
                Self::Plain(_) if b == b'\\' => {
                    at += 2;
                    continue;
                }
                Self::Plain(quote) if b == quote => return at,
                // Closed by as many backslashes before the quote as opened it; more is a quote escaped once more inside the string, and the value goes on.
                Self::Escaped(quote, slashes) if b == quote => {
                    let before = bytes.get(inside..at).unwrap_or_default();
                    let run = before.iter().rev().take_while(|b| **b == b'\\').count();
                    steps::add(run);
                    if run == slashes {
                        return at - slashes;
                    }
                }
                Self::Encoded if b == b'%' && bytes.get(at + 1..at + 3) == Some(b"22") => {
                    return at;
                }
                _ => {}
            }
            at += 1;
        }
        bytes.len()
    }
}

/// What joins a secret name to its value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Separator {
    /// `name: value`, also URL-encoded (`%3A`).
    Colon,
    /// `name=value`, also URL-encoded (`%3D`).
    Equals,
    /// `("name", "value")`, as `Debug` writes a tuple.
    Pair,
    /// `--flag value`.
    Space,
}

/// `at` past the quote that starts there, if any.
fn past_quote(bytes: &[u8], at: usize) -> usize {
    at + Quote::at(bytes, at).map_or(0, Quote::len)
}

/// Where the value of a header object written name first starts, after the name ending at `after_name` (its closing quote included): `"Cookie", "value": ..`, `Cookie, value: ..` or `Cookie&value=..`. With the separator after `value`.
fn name_first_value(bytes: &[u8], after_name: usize) -> Option<(Separator, usize)> {
    let at = skip_blanks(bytes, after_name, true);
    if !matches!(bytes.get(at), Some(b',' | b'&' | b';')) {
        return None;
    }
    let key = past_quote(bytes, skip_blanks(bytes, at + 1, true));
    let key_end = scan_until(bytes, key, |b| !is_name_byte(b));
    if !bytes.get(key..key_end)?.eq_ignore_ascii_case(VALUE_KEY) {
        return None;
    }
    let at = skip_blanks(bytes, past_quote(bytes, key_end), true);
    match bytes.get(at)? {
        b':' => Some((Separator::Colon, at + 1)),
        b'=' => Some((Separator::Equals, at + 1)),
        _ => None,
    }
}

/// Where the quoted value of a header object written value first is, after its `value` key ending at `key_end`, when the `name` after it is a secret's: `{"value": "..", "name": "Cookie"}`.
fn value_named_after(bytes: &[u8], key_end: usize) -> Option<(usize, usize)> {
    let at = skip_blanks(bytes, past_quote(bytes, key_end), true);
    if !matches!(bytes.get(at), Some(b':' | b'=')) {
        return None;
    }
    let start = skip_blanks(bytes, at + 1, true);
    let quote = Quote::at(bytes, start)?;
    let inside = start + quote.len();
    let close = quote.close(bytes, inside);
    let at = skip_blanks(bytes, close + quote.len(), true);
    if bytes.get(at) != Some(&b',') {
        return None;
    }
    let key = past_quote(bytes, skip_blanks(bytes, at + 1, true));
    let key_end = scan_until(bytes, key, |b| !is_name_byte(b));
    if !bytes.get(key..key_end)?.eq_ignore_ascii_case(NAME_KEY) {
        return None;
    }
    let at = skip_blanks(bytes, past_quote(bytes, key_end), true);
    if !matches!(bytes.get(at), Some(b':' | b'=')) {
        return None;
    }
    let name = past_quote(bytes, skip_blanks(bytes, at + 1, true));
    let name_end = scan_until(bytes, name, |b| !is_name_byte(b));
    let kind = kind_of(bytes.get(name..name_end)?)?;
    (kind != Kind::LinkField && close > inside).then_some((inside, close))
}

/// Where the value after the secret name ending at `name_end` starts and ends: past a closing quote of the name, blanks (newlines too after a quoted name, as pretty-printed JSON has them), the separator (or a header object's `value` key, `"Cookie", "value": ..`), blanks, and `Debug` wrappers such as `Some(`; then inside its quotes, to its closing bracket, or else to the end of its field (after `:`, and for a [`Kind::WholeField`] name) or of its word. `None` when no value follows, or it is `None`.
fn secret_value(
    bytes: &[u8],
    name_start: usize,
    name_end: usize,
    kind: Kind,
) -> Option<(usize, usize)> {
    // `<password>value</password>`.
    let tagged = name_start
        .checked_sub(1)
        .is_some_and(|before| bytes.get(before) == Some(&b'<'))
        && bytes.get(name_end) == Some(&b'>');
    if tagged {
        let end = scan_until(bytes, name_end + 1, |b| matches!(b, b'<' | b'\n' | b'\r'));
        return (end > name_end + 1).then_some((name_end + 1, end));
    }
    let name_quote = Quote::at(bytes, name_end);
    let after_name = name_end + name_quote.map_or(0, Quote::len);
    let quoted_name = name_quote.is_some();
    let at = skip_blanks(bytes, after_name, quoted_name);
    let (separator, value_from) = match name_first_value(bytes, after_name) {
        Some(found) => found,
        None => match *bytes.get(at)? {
            b':' if bytes.get(at + 1) != Some(&b':') => (Separator::Colon, at + 1),
            b'=' if bytes.get(at + 1) == Some(&b'>') => (Separator::Equals, at + 2),
            b'=' => (Separator::Equals, at + 1),
            b'%' => match bytes.get(at + 1..at + 3)? {
                code if code.eq_ignore_ascii_case(b"3a") => (Separator::Colon, at + 3),
                code if code.eq_ignore_ascii_case(b"3d") => (Separator::Equals, at + 3),
                _ => return None,
            },
            b',' if quoted_name => (Separator::Pair, at + 1),
            _ if kind == Kind::Flag && at > after_name => (Separator::Space, at),
            _ => return None,
        },
    };
    if kind == Kind::LinkField && separator != Separator::Colon {
        return None;
    }
    // Not `https://`: a link is no name and value.
    if separator != Separator::Space && bytes.get(value_from..value_from + 2) == Some(b"//") {
        return None;
    }
    // Pretty `Debug` (`{:#?}`) and JSON may put a quoted or wrapped value on the next line.
    let next_line = skip_blanks(bytes, value_from, true);
    let mut start = if quoted_name
        || Quote::at(bytes, next_line).is_some()
        || wrapper_end(bytes, next_line).is_some()
        || opens_a_bracket(bytes, next_line)
    {
        next_line
    } else {
        skip_blanks(bytes, value_from, false)
    };
    for _ in 0..MAX_WRAPPERS {
        match wrapper_end(bytes, start) {
            Some(end) => start = skip_blanks(bytes, end + 1, true),
            None => break,
        }
    }
    if let Some(quote) = Quote::at(bytes, start) {
        let inside = start + quote.len();
        let close = quote.close(bytes, inside);
        return (close > inside).then_some((inside, close));
    }
    if kind != Kind::LinkField && opens_a_bracket(bytes, start) {
        return Some((start, bracket_end(bytes, start)));
    }
    let is_none = bytes.get(start..start + 4) == Some(b"None")
        && !bytes.get(start + 4).is_some_and(|b| is_name_byte(*b));
    if separator == Separator::Pair || kind == Kind::LinkField || is_none {
        return None;
    }
    let word_or_field_end = match (kind, separator) {
        (Kind::WholeLine, _) => line_end(bytes, start, true),
        (_, Separator::Space) => scan_until(bytes, start, |b| b.is_ascii_whitespace()),
        (Kind::WholeField, _) | (Kind::Password, Separator::Colon) => field_end(bytes, start, true),
        (_, Separator::Colon) => field_end(bytes, start, false),
        _ => word_end(bytes, start),
    };
    let end = past_scheme_credential(bytes, start, word_or_field_end);
    (end > start).then_some((start, end))
}

/// `end`, or past the credential when the value from `start` to `end` is an authorization scheme word alone (`token=Bearer abc`, `X-Auth-Token: Bearer` with the credential on the next line): the scheme says the word after it is the secret.
fn past_scheme_credential(bytes: &[u8], start: usize, end: usize) -> usize {
    if !bytes.get(start..end).is_some_and(is_scheme_word) {
        return end;
    }
    let credential = past_scheme_gap(bytes, end);
    if credential == end {
        return end;
    }
    let after = credential_end(bytes, credential);
    if after > credential { after } else { end }
}

/// Whether a list or an object opens at `at`.
fn opens_a_bracket(bytes: &[u8], at: usize) -> bool {
    matches!(bytes.get(at), Some(b'[' | b'{'))
}

/// Just past the bracket that closes the list or object opening at `start`, skipping what is quoted; the end of the text when it is never closed.
fn bracket_end(bytes: &[u8], start: usize) -> usize {
    let mut depth = 0_usize;
    let mut at = start;
    while let Some(&b) = bytes.get(at) {
        steps::add(1);
        match b {
            b'[' | b'{' => depth += 1,
            b']' | b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return at + 1;
                }
            }
            b'"' | b'\'' => {
                at = Quote::Plain(b).close(bytes, at + 1);
            }
            _ => {}
        }
        at += 1;
    }
    bytes.len()
}

/// Where the `(` is of a `Debug` wrapper such as `Some(` or `Secret(` that starts at `at`.
fn wrapper_end(bytes: &[u8], at: usize) -> Option<usize> {
    let end = scan_until(bytes, at, |b| !(b.is_ascii_alphanumeric() || b == b'_'));
    (end > at
        && bytes.get(at).is_some_and(u8::is_ascii_alphabetic)
        && bytes.get(end) == Some(&b'('))
    .then_some(end)
}

/// Where the line after the line break at `at` starts, when that line continues the field before it: it is neither blank nor starts a field of its own (`User-Agent:`), as a folded header line or a wrapped credential does.
fn continued_line(bytes: &[u8], at: usize) -> Option<usize> {
    let next = match bytes.get(at..at + 2)? {
        b"\r\n" => at + 2,
        _ => at + 1,
    };
    if matches!(bytes.get(next), None | Some(b'\r' | b'\n')) {
        return None;
    }
    let word_end = scan_until(bytes, next, |b| {
        !(b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
    });
    let at_colon = scan_until(bytes, word_end, |b| b != b' ' && b != b'\t');
    let starts_a_field = word_end > next && bytes.get(at_colon) == Some(&b':');
    (!starts_a_field).then_some(next)
}

/// The end of the line from `start`, or ` / ` (how a program's last lines are joined in one log line); with `continued`, over the lines that continue it.
fn line_end(bytes: &[u8], start: usize, continued: bool) -> usize {
    let mut at = start;
    loop {
        at = scan_until(bytes, at, |b| matches!(b, b'\n' | b'\r' | b' '));
        match bytes.get(at) {
            Some(b' ') if bytes.get(at + 1..at + 3) == Some(b"/ ") => return at,
            Some(b' ') => at += 1,
            Some(_) if continued => match continued_line(bytes, at) {
                Some(next) => at = skip_blanks(bytes, next, false),
                None => return at,
            },
            _ => return at,
        }
    }
}

/// `at` past spaces and tabs, and newlines too when `newlines`.
fn skip_blanks(bytes: &[u8], at: usize, newlines: bool) -> usize {
    scan_until(bytes, at, |b| {
        !(b == b' ' || b == b'\t' || (newlines && (b == b'\n' || b == b'\r')))
    })
}

/// The end of a field from `start`: the end of the line, ` / ` (how a program's last lines are joined in one log line), or a space before another field's `name=` (one after `;` or `,` belongs to a cookie header's next pair); with `continued`, over the lines that continue it.
fn field_end(bytes: &[u8], start: usize, continued: bool) -> usize {
    let mut at = start;
    loop {
        at = scan_until(bytes, at, |b| matches!(b, b'\n' | b'\r' | b' '));
        match bytes.get(at) {
            Some(b' ') => {
                let after_a_pair = at > start && matches!(bytes.get(at - 1), Some(b';' | b','));
                if bytes.get(at + 1..at + 3) == Some(b"/ ")
                    || (!after_a_pair && starts_a_named_value(bytes, at + 1))
                {
                    return at;
                }
                at += 1;
            }
            Some(_) if continued => match continued_line(bytes, at) {
                Some(next) => at = skip_blanks(bytes, next, false),
                None => return at,
            },
            _ => return at,
        }
    }
}

/// The end of a word from `start`: whitespace, `,;)]}`, a quote, `%26` or `%22`, or an `&` before another `name=` (an `&` inside a password belongs to it).
fn word_end(bytes: &[u8], start: usize) -> usize {
    let mut at = start;
    loop {
        at = scan_until(bytes, at, |b| {
            b.is_ascii_whitespace()
                || matches!(
                    b,
                    b',' | b';' | b')' | b']' | b'}' | b'"' | b'\'' | b'\\' | b'&' | b'%'
                )
        });
        match bytes.get(at) {
            Some(b'&') if !starts_a_named_value(bytes, at + 1) => at += 1,
            Some(b'%') if !matches!(bytes.get(at + 1..at + 3), Some(b"26" | b"22")) => at += 1,
            _ => return at,
        }
    }
}

/// Whether a `name=` starts at `at`.
fn starts_a_named_value(bytes: &[u8], at: usize) -> bool {
    let end = scan_until(bytes, at, |b| !is_name_byte(b));
    end > at && bytes.get(end) == Some(&b'=')
}
