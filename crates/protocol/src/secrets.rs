//! What a secret is, for both redactions: the log's (`wirecatch-logging`) and the extension's diagnostics (`apps/extension/src/lib/diag`). The rules are [`is_secret_name`], [`names_a_password`], [`looks_like_a_credential`] and [`is_service_key`], ported into `@wirecatch/protocol`; the words, header names and scheme words they and the redactions read are generated as constants. The cases both the rules and the two redactions are tested against live in one file both languages load, `crates/test-support/data/redaction.json` (D40.10).
//!
//! What both redactions apply besides those rules:
//!
//! - The value of a [`COOKIE_HEADERS`] or [`AUTHORIZATION_HEADERS`] header line, and a password's after `:`, runs to the end of its field, and on over the lines after it that continue it (that start neither a field of their own, `Name:`, nor a blank line). Any other secret's value after `:` runs to the end of its field on its line: the line's end, ` / `, or a space before another `name=` that does not follow `;` or `,`. After `=` it runs to the end of its word, an `&` no `name=` follows included.
//! - After one of [`AUTH_SCHEMES`] the next word is a credential: any word after `Bearer` spelt as a header spells it, and after the others (`bearer` in lowercase included) a word that [`looks_like_a_credential`], since `basic` and `bearer` are also common words. The credential may follow on the next line, and a scheme word right after `=` or `:` counts. When a secret's value starts with a scheme word, the credential after it is part of the value (`X-Auth-Token: Bearer abc`, `token=Bearer abc`).
//!
//! Each redaction finds more than this on its own, as D40.10 records: the log reads Rust's `Debug` output and command lines, and the extension reduces addresses to their sites, hides paths and e-mail addresses, decodes percent-encoded text and hides long opaque runs.

use crate::exported::exported_consts;

/// What a hidden value reads as in what Rust writes: the log, and the `Debug` of the protocol's types that may hold a secret. The extension's diagnostics write `[redacted]`, which reads better in a sentence a person copies.
pub const HIDDEN: &str = "***";

exported_consts! {
    /// Words that make a name a secret's when one of its segments is the word (see the module documentation for how a name is split). `key` alone counts, so `Invalid key: 5` loses its 5: a bare `key=` pair is most often an API key.
    pub const SECRET_WORDS: &[&str] = &[
        "token",
        "secret",
        "password",
        "passwd",
        "passphrase",
        "pass",
        "pwd",
        "passcode",
        "session",
        "sessionid",
        "sessid",
        "jsessionid",
        "phpsessid",
        "sid",
        "sig",
        "signature",
        "hdnts",
        "key",
        "apikey",
        "accesskey",
        "privatekey",
        "private",
        "auth",
        "authentication",
        "authorization",
        "bearer",
        "credential",
        "credentials",
        "cookie",
        "cookies",
        "csrf",
        "xsrf",
        "jwt",
        "otp",
    ];
    /// Endings that make a segment run together from words a secret's: `accesstoken`, `clientsecret`, `oauth`, `proxyauthorization`, `xapikey`. Only as an ending, so `tokens` and `author` keep their values.
    pub const SECRET_ENDINGS: &[&str] = &[
        "token",
        "secret",
        "password",
        "passwd",
        "passphrase",
        "passcode",
        "auth",
        "authorization",
        "credential",
        "credentials",
        "cookie",
        "apikey",
        "accesskey",
        "privatekey",
        "sessionid",
        "sessid",
        "signature",
    ];
    /// Secret words that name a password, whose value after a `:` may be several words (`password: correct horse battery staple`).
    pub const PASSWORD_WORDS: &[&str] = &["password", "passwd", "passphrase", "pass", "pwd", "passcode"];
    /// Headers that carry the browser's cookies, compared in lowercase. A cookie header's value holds `;`, `=` and quotes, and runs to the end of its field.
    pub const COOKIE_HEADERS: &[&str] = &["cookie", "set-cookie"];
    /// Headers that carry a sign-in, compared in lowercase. Their value holds spaces and, for `Digest`, `name=value` pairs of its own, and runs to the end of its field.
    pub const AUTHORIZATION_HEADERS: &[&str] = &["authorization", "proxy-authorization"];
    /// Authorization schemes whose next word is a credential, compared in lowercase. `Digest` is not one: its credential is a list of named pairs.
    pub const AUTH_SCHEMES: &[&str] = &["bearer", "basic", "negotiate", "ntlm"];
    /// Shortest word after a scheme other than `Bearer` that is taken for a credential.
    pub const CREDENTIAL_MIN_CHARS: usize = 8;
    /// How services start their keys, compared as written: those of Stripe, Anthropic, `OpenAI`, `GitHub`, `GitLab`, npm, Slack, AWS and Google.
    pub const KEY_PREFIXES: &[&str] = &[
        "sk_live_",
        "sk_test_",
        "sk-live-",
        "sk-test-",
        "pk_live_",
        "pk_test_",
        "rk_live_",
        "rk_test_",
        "sk-ant-",
        "sk-proj-",
        "ghp_",
        "gho_",
        "ghu_",
        "ghs_",
        "ghr_",
        "github_pat_",
        "glpat-",
        "npm_",
        "xoxa-",
        "xoxb-",
        "xoxp-",
        "xoxr-",
        "xoxs-",
        "AKIA",
        "ASIA",
        "AIza",
    ];
    /// Fewest characters after a key prefix for a word to be taken for a key, so prose that starts the same way (`AKIA` alone) is kept.
    pub const KEY_MIN_REST: usize = 16;
}

/// Whether a byte splits a name into segments.
fn splits_a_name(b: u8) -> bool {
    matches!(b, b'-' | b'_' | b'.')
}

/// The segments of `name`: split at `-`, `_` and `.`, and where a lowercase letter or a digit is followed by a capital (`sessionKey` is two, `PHPSESSID` one).
fn segments(name: &str) -> impl Iterator<Item = &str> {
    let bytes = name.as_bytes();
    let mut start = 0;
    std::iter::from_fn(move || {
        while bytes.get(start).copied().is_some_and(splits_a_name) {
            start += 1;
        }
        if start >= bytes.len() {
            return None;
        }
        let mut end = start + 1;
        while let Some(&b) = bytes.get(end) {
            let before = bytes.get(end - 1).copied().unwrap_or_default();
            if splits_a_name(b)
                || (b.is_ascii_uppercase()
                    && (before.is_ascii_lowercase() || before.is_ascii_digit()))
            {
                break;
            }
            end += 1;
        }
        // Both ends sit before an ASCII byte or at the end, so on character boundaries.
        let segment = name.get(start..end);
        start = end;
        segment
    })
}

/// Whether `text` ends with `ending`, in any case.
fn ends_with_ignoring_case(text: &str, ending: &str) -> bool {
    text.len()
        .checked_sub(ending.len())
        .and_then(|from| text.as_bytes().get(from..))
        .is_some_and(|tail| tail.eq_ignore_ascii_case(ending.as_bytes()))
}

/// Whether the value named `name` is a secret: one of the name's segments (split at `-`, `_`, `.` and camelCase steps) is one of [`SECRET_WORDS`] or ends with one of [`SECRET_ENDINGS`], in any case. `X-Amz-Security-Token`, `apiKey`, `PHPSESSID`, `authHeader` and `accesstoken` are, `author`, `tokens`, `monkey` and `keyframes` are not.
#[must_use]
pub fn is_secret_name(name: &str) -> bool {
    segments(name).any(|segment| {
        SECRET_WORDS
            .iter()
            .any(|word| segment.eq_ignore_ascii_case(word))
            || SECRET_ENDINGS
                .iter()
                .any(|ending| ends_with_ignoring_case(segment, ending))
    })
}

/// Whether the value named `name` is a password, which after a `:` may be several words: one of its segments is one of [`PASSWORD_WORDS`].
#[must_use]
pub fn names_a_password(name: &str) -> bool {
    segments(name).any(|segment| {
        PASSWORD_WORDS
            .iter()
            .any(|word| segment.eq_ignore_ascii_case(word))
    })
}

/// Whether a word after an authorization scheme that is also a common word (`basic`) looks like a credential rather than prose: at least [`CREDENTIAL_MIN_CHARS`] characters, with a digit, a `+`, `/` or `=`, or a capital after the first character.
#[must_use]
pub fn looks_like_a_credential(word: &str) -> bool {
    word.chars().count() >= CREDENTIAL_MIN_CHARS
        && word.chars().enumerate().any(|(i, c)| {
            c.is_ascii_digit() || matches!(c, '+' | '/' | '=') || (i > 0 && c.is_ascii_uppercase())
        })
}

/// Whether `word` is a service's key: it starts with one of [`KEY_PREFIXES`] and goes on for at least [`KEY_MIN_REST`] characters, a digit or a capital among them.
#[must_use]
pub fn is_service_key(word: &str) -> bool {
    KEY_PREFIXES.iter().any(|prefix| {
        word.strip_prefix(prefix).is_some_and(|rest| {
            rest.chars().count() >= KEY_MIN_REST
                && rest
                    .chars()
                    .any(|c| c.is_ascii_digit() || c.is_ascii_uppercase())
        })
    })
}

#[cfg(test)]
mod tests {
    use wirecatch_test_support::redaction::rule_cases;

    use super::*;

    /// Each rule against its cases in `crates/test-support/data/redaction.json`, which the TypeScript port reads too.
    #[test]
    fn each_rule_answers_its_shared_cases() {
        type Rule = fn(&str) -> bool;
        let rules: [(&str, Rule); 4] = [
            ("is_secret_name", is_secret_name),
            ("names_a_password", names_a_password),
            ("looks_like_a_credential", looks_like_a_credential),
            ("is_service_key", is_service_key),
        ];
        for (name, rule) in rules {
            for case in rule_cases(name) {
                assert_eq!(rule(&case.input), case.answer, "{name}({:?})", case.input);
            }
        }
    }

    #[test]
    fn the_lists_are_written_as_they_are_compared() {
        for list in [
            SECRET_WORDS,
            SECRET_ENDINGS,
            PASSWORD_WORDS,
            COOKIE_HEADERS,
            AUTHORIZATION_HEADERS,
            AUTH_SCHEMES,
        ] {
            for word in list {
                assert_eq!(
                    *word,
                    word.to_ascii_lowercase(),
                    "{word:?} is compared in lowercase"
                );
            }
        }
        for word in PASSWORD_WORDS {
            assert!(
                SECRET_WORDS.contains(word),
                "{word:?} is a password word but no secret word"
            );
        }
        for header in COOKIE_HEADERS.iter().chain(AUTHORIZATION_HEADERS) {
            assert!(is_secret_name(header), "{header:?} is no secret's name");
        }
    }
}
