//! Text with its secrets hidden, at two strengths.
//!
//! - For display ([`url_for_display`], [`text_for_display`]): a person sees their own links, so only the password in a link (`https://user:secret@host/`) is hidden, in every link of the text, links inside links included. The code keeps and uses the real link.
//! - For logs ([`text_for_log`]): a log file is read by support and attached to bug reports, so it never holds a secret (docs/ENGINEERING.md section 6). Besides passwords in links, it hides every query value and fragment of a link or of a request's path (a signed link carries its token there), the word after an authorization scheme (`Bearer abc`), every JWT and service key, and the value of every name that marks a secret (a cookie, an authorization header, a password, token, key, signature or session, as `wirecatch_protocol::secrets` decides it for this redaction and the extension's), however the value is written: `name: value`, `name=value`, quoted (also with the quotes escaped, as `Debug` writes a string, or URL-encoded), a list or object to its closing bracket, a header line over the lines that continue it, a header object name first or value first (`{"name": "Cookie", "value": ..}`), as a `Debug` tuple or struct field (`Some("..")` included, and the user name, query and fragment of `url::Url`'s), or after a command-line flag (`--password value`).
//!
//! What cannot be recognized in text, such as a token in the middle of a link's path, is not hidden: code still never logs a secret on purpose, and this is the net under it. Each pass looks at every byte a bounded number of times, so a long line costs a few plain copies of it, whatever it holds.

/// What a hidden secret reads as.
pub const HIDDEN: &str = wirecatch_protocol::secrets::HIDDEN;

mod named;
mod scan;
#[cfg(test)]
mod tests;

use named::hide_named_secrets;
#[cfg(feature = "setup")]
pub(crate) use named::names_a_secret;
use scan::{NextAt, find, scan_until, steps};
use wirecatch_protocol::secrets::{AUTH_SCHEMES, is_service_key, looks_like_a_credential};

/// `url` with the password in it, if any, replaced by [`HIDDEN`] (`https://ada:***@example.com/file`), and so in any link inside it. A link with a user name but no password hides the name the same way, since some sites put a token there. Anything else is returned as it is.
#[must_use]
pub fn url_for_display(url: &str) -> String {
    hide_link_credentials(url)
}

/// [`url_for_display`] for an optional link.
#[must_use]
pub fn optional_url_for_display(url: Option<&str>) -> Option<String> {
    url.map(url_for_display)
}

/// `text` (an error message, say) with the password of every link in it hidden the way [`url_for_display`] does.
#[must_use]
pub fn text_for_display(text: &str) -> String {
    hide_link_credentials(text)
}

/// `text` (a log line, or what a program wrote) with every secret it shows hidden: see the [module documentation](self).
#[must_use]
pub fn text_for_log(text: &str) -> String {
    let text = hide_link_credentials(text);
    let text = hide_queries(&text);
    let text = hide_scheme_credentials(&text);
    hide_named_secrets(&text)
}

/// Whether a byte ends a link written in text.
fn ends_a_link(b: u8) -> bool {
    b.is_ascii_whitespace() || matches!(b, b'"' | b'\'' | b'<' | b'>' | b'`' | b'\\')
}

/// `text` with the user information of every link hidden: the password (`user:***@`), or the user name alone when there is no password. A password may hold `/`, `?` or `#`, as people paste them: a link whose host part has a `:` followed by no port takes its user information up to the first `@` before the link ends. After each link's host the scan goes on, so a link inside another's path or query is seen too.
fn hide_link_credentials(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    // Everything before `copied` is in `out`.
    let mut copied = 0;
    let mut from = 0;
    let mut link_end = NextAt::default();
    let mut next_at_sign = NextAt::default();
    while let Some(found) = find(bytes, from, b"://") {
        let start = found + 3;
        let end = link_end.from(start, |at| scan_until(bytes, at, ends_a_link));
        let host_end = scan_until(bytes.get(..end).unwrap_or(bytes), start, |b| {
            matches!(b, b'/' | b'?' | b'#')
        });
        let host = bytes.get(start..host_end).unwrap_or_default();
        let at_sign = match host.iter().rposition(|b| *b == b'@') {
            Some(at) => Some(start + at),
            None if may_hide_a_password(host) => {
                Some(next_at_sign.from(host_end, |at| scan_until(bytes, at, |b| b == b'@')))
                    .filter(|at| *at < end)
            }
            None => None,
        };
        let Some(at_sign) = at_sign else {
            from = host_end.max(start);
            continue;
        };
        let user_information = &text[start..at_sign];
        out.push_str(&text[copied..start]);
        if let Some(colon) = user_information.find(':') {
            out.push_str(&user_information[..=colon]);
        }
        out.push_str(HIDDEN);
        copied = at_sign;
        from = at_sign + 1;
    }
    out.push_str(&text[copied..]);
    out
}

/// Whether a link's host part without an `@` (`user:pa` of `https://user:pa/ss@host`) may be the start of user information whose password holds a `/`, `?` or `#`: it has a `:` followed by something other than a port, and is no IPv6 address.
fn may_hide_a_password(host: &[u8]) -> bool {
    if host.first() == Some(&b'[') {
        return false;
    }
    host.iter().rposition(|b| *b == b':').is_some_and(|colon| {
        let after = host.get(colon + 1..).unwrap_or_default();
        !after.is_empty() && !after.iter().all(u8::is_ascii_digit)
    })
}

/// `text` with the query values and the fragment of every link, and of every path written alone (`GET /v.m3u8?st=abc HTTP/1.1`), hidden.
fn hide_queries(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        if ends_a_link(b) {
            steps::add(1);
            i += 1;
            continue;
        }
        let end = scan_until(bytes, i, ends_a_link);
        let word = bytes.get(i..end).unwrap_or_default();
        let query = match find(word, 0, b"://") {
            Some(scheme) => Some(scan_until(word, scheme + 3, |b| b == b'?' || b == b'#')),
            None if b == b'/' => Some(scan_until(word, 0, |b| b == b'?' || b == b'#')),
            None => None,
        }
        .filter(|query| *query < word.len());
        if let Some(query) = query {
            out.push_str(&text[copied..i + query]);
            push_query_hidden(&mut out, &text[i + query..end]);
            copied = end;
        }
        i = end;
    }
    out.push_str(&text[copied..]);
    out
}

/// Writes a link's query and fragment (`rest` starts at its `?` or `#`) with each query value as [`HIDDEN`] (a parameter without `=` wholly), and the fragment as [`HIDDEN`].
fn push_query_hidden(out: &mut String, rest: &str) {
    let (before_fragment, fragment) = match rest.split_once('#') {
        Some((before, fragment)) => (before, Some(fragment)),
        None => (rest, None),
    };
    if let Some(query) = before_fragment.strip_prefix('?') {
        out.push('?');
        for (i, parameter) in query.split('&').enumerate() {
            if i > 0 {
                out.push('&');
            }
            match parameter.split_once('=') {
                Some((name, "")) => {
                    out.push_str(name);
                    out.push('=');
                }
                Some((name, _)) => {
                    out.push_str(name);
                    out.push('=');
                    out.push_str(HIDDEN);
                }
                None if parameter.is_empty() => {}
                None => out.push_str(HIDDEN),
            }
        }
    }
    if let Some(fragment) = fragment {
        out.push('#');
        if !fragment.is_empty() {
            out.push_str(HIDDEN);
        }
    }
}

/// Whether a byte can be part of a credential word: base64, base64url and the other characters of an HTTP `token68`.
fn is_credential_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~' | b'+' | b'/' | b'=')
}

/// The end of the credential word from `start`: its credential bytes, but not past an `=` with a letter or digit after it, which joins a name to its value (`token=Bearer`) where base64 padding ends a word.
fn credential_word_end(bytes: &[u8], start: usize) -> usize {
    let mut at = start;
    loop {
        at = scan_until(bytes, at, |b| !is_credential_byte(b) || b == b'=');
        if bytes.get(at) != Some(&b'=') || bytes.get(at + 1).is_some_and(u8::is_ascii_alphanumeric)
        {
            return at;
        }
        at = scan_until(bytes, at, |b| b != b'=');
    }
}

/// Where the credential after an authorization scheme word ending at `end` starts: past spaces and tabs and at most one line break, as a header folds it. `end` itself when nothing separates them.
pub(super) fn past_scheme_gap(bytes: &[u8], end: usize) -> usize {
    let at = scan_until(bytes, end, |b| b != b' ' && b != b'\t');
    let at = match bytes.get(at..) {
        Some([b'\r', b'\n', ..]) => at + 2,
        Some([b'\r' | b'\n', ..]) => at + 1,
        _ => at,
    };
    scan_until(bytes, at, |b| b != b' ' && b != b'\t')
}

/// Whether `word` is one of the authorization scheme words, in any case.
pub(super) fn is_scheme_word(word: &[u8]) -> bool {
    AUTH_SCHEMES
        .iter()
        .any(|scheme| word.eq_ignore_ascii_case(scheme.as_bytes()))
}

/// The end of the credential word from `start`, an already hidden one included; `start` when there is none.
pub(super) fn credential_end(bytes: &[u8], start: usize) -> usize {
    if bytes.get(start..start + HIDDEN.len()) == Some(HIDDEN.as_bytes()) {
        start + HIDDEN.len()
    } else {
        scan_until(bytes, start, |b| !is_credential_byte(b))
    }
}

/// `text` with every JWT and service key, and the word after an authorization scheme (see `wirecatch_protocol::secrets`), hidden: after `Bearer` spelt as a header spells it any word, after the other schemes in any case a word that looks like a credential.
fn hide_scheme_credentials(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        // A word starts after anything but a credential byte, or after the `=` of `name=value`.
        if !b.is_ascii_alphanumeric()
            || i.checked_sub(1)
                .and_then(|before| bytes.get(before))
                .is_some_and(|before| is_credential_byte(*before) && *before != b'=')
        {
            steps::add(1);
            i += 1;
            continue;
        }
        let end = credential_word_end(bytes, i);
        let word = &text[i..end];
        if is_jwt(word) || is_service_key(word) {
            out.push_str(&text[copied..i]);
            out.push_str(HIDDEN);
            copied = end;
            i = end;
            continue;
        }
        if is_scheme_word(word.as_bytes()) {
            let start = past_scheme_gap(bytes, end);
            let credential_end = scan_until(bytes, start, |b| !is_credential_byte(b));
            let credential = &text[start..credential_end];
            if start > end
                && !credential.is_empty()
                && (word == "Bearer" || looks_like_a_credential(credential))
            {
                out.push_str(&text[copied..start]);
                out.push_str(HIDDEN);
                copied = credential_end;
                i = credential_end;
                continue;
            }
        }
        i = end;
    }
    out.push_str(&text[copied..]);
    out
}

/// Whether `word` is a JSON Web Token: `eyJ` (the start of an encoded `{"`), then at least three base64url parts joined by `.`.
fn is_jwt(word: &str) -> bool {
    let mut parts = word.split('.');
    let is_part = |part: Option<&str>| {
        part.is_some_and(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        })
    };
    word.starts_with("eyJ")
        && is_part(parts.next())
        && is_part(parts.next())
        && parts.next().is_some()
}
