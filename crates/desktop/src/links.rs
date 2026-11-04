//! Links in pasted or typed text.

use std::sync::LazyLock;

use regex::Regex;
use url::Url;
use wirecatch_engine::url::{FTP_SCHEMES, WEB_SCHEMES};

/// A link in any scheme the engine takes, up to the first character no link holds.
static LINK: LazyLock<Option<Regex>> = LazyLock::new(|| {
    let schemes = WEB_SCHEMES.iter().chain(&FTP_SCHEMES).copied();
    let schemes = schemes.collect::<Vec<_>>().join("|");
    Regex::new(&format!(r#"(?i)\b(?:{schemes})://[^\s<>"`]+"#)).ok()
});

/// Every link in `text`, each once, in order. Punctuation that ends a sentence after a link is
/// not part of it.
pub fn extract(text: &str) -> Vec<String> {
    let Some(link) = LINK.as_ref() else {
        return Vec::new();
    };
    let mut found: Vec<String> = Vec::new();
    let mut from = 0;
    while let Some(matched) = link.find_at(text, from) {
        let raw = up_to_closing_quote(matched.as_str()).unwrap_or(matched.as_str());
        from = matched.start() + raw.len();
        if let Ok(url) = Url::parse(without_punctuation(raw)) {
            let url = String::from(url);
            if !found.contains(&url) {
                found.push(url);
            }
        }
    }
    found
}

/// `raw` up to a quote that closes around it: a list of quoted links reads as one match.
fn up_to_closing_quote(raw: &str) -> Option<&str> {
    raw.match_indices('\'')
        .find(|(at, _)| raw[at + 1..].starts_with([',', ';', ')', ']']))
        .map(|(at, _)| &raw[..at])
}

/// `raw` without the punctuation after it. A closing bracket stays when its opener is inside.
fn without_punctuation(raw: &str) -> &str {
    let mut link = raw;
    while let Some(last) = link.chars().last() {
        let ends = match last {
            ',' | '.' | ';' | ':' | '!' | '?' | '\'' => true,
            ')' => link.matches(')').count() > link.matches('(').count(),
            ']' => link.matches(']').count() > link.matches('[').count(),
            _ => false,
        };
        if !ends {
            break;
        }
        link = &link[..link.len() - last.len_utf8()];
    }
    link
}

/// Lines with text but no link, which the Add dialog leaves out.
pub fn lines_without_links(text: &str) -> usize {
    text.lines()
        .filter(|line| !line.trim().is_empty() && extract(line).is_empty())
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use wirecatch_shell::launch::links_from_args;

    #[test]
    fn links_are_found_once_without_the_sentence_around_them() {
        let text = "Get https://example.com/a.zip, then (https://example.com/b.iso).\nhttps://example.com/a.zip";
        assert_eq!(
            extract(text),
            ["https://example.com/a.zip", "https://example.com/b.iso"]
        );
    }

    #[test]
    fn lines_without_a_link_are_counted() {
        assert_eq!(
            lines_without_links("https://example.com/a\nnotes\n\nftp://example.com/b\nmore"),
            2
        );
    }

    #[test]
    fn a_link_keeps_its_apostrophe_and_its_own_brackets() {
        for link in [
            "https://example.com/Bob's%20file.zip",
            "https://example.com/report_(final)",
            "https://example.com/list[1]",
        ] {
            assert_eq!(extract(link), [link]);
            assert_eq!(extract(&format!("See ({link}).")), [link]);
        }
    }

    #[test]
    fn quotes_around_a_link_are_not_part_of_it() {
        assert_eq!(
            extract("'https://example.com/a','https://example.com/b'"),
            ["https://example.com/a", "https://example.com/b"]
        );
    }

    #[test]
    fn the_add_dialog_and_the_command_line_take_the_same_schemes() {
        for scheme in WEB_SCHEMES.iter().chain(&FTP_SCHEMES) {
            let link = format!("{scheme}://example.com/a");
            assert_eq!(extract(&link), [link.as_str()], "typed");
            assert_eq!(
                links_from_args(["app", link.as_str()]),
                [link.as_str()],
                "launch"
            );
        }
    }

    #[test]
    fn a_command_line_link_with_a_space_reaches_the_dialog_whole() {
        let links = links_from_args(["app", "https://example.com/a b.zip"]);
        assert_eq!(
            extract(&links.join("\n")),
            ["https://example.com/a%20b.zip"]
        );
    }
}
