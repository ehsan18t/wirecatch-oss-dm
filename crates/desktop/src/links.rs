//! Links in pasted or typed text.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
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
/// not part of it. One pass over `text`: it may be a paste of any size, and a window reads it on
/// every redraw, through [`Kept`].
pub fn extract(text: &str) -> Vec<String> {
    let Some(link) = LINK.as_ref() else {
        return Vec::new();
    };
    let mut seen = HashSet::new();
    let mut found = Vec::new();
    for piece in between_closing_quotes(text) {
        for raw in link.find_iter(piece) {
            if let Ok(url) = Url::parse(without_punctuation(raw.as_str())) {
                let url = String::from(url);
                if seen.insert(url.clone()) {
                    found.push(url);
                }
            }
        }
    }
    found
}

/// `text` cut at each quote that closes around something: a list of quoted links would otherwise
/// read as one link.
fn between_closing_quotes(text: &str) -> Vec<&str> {
    let mut pieces = Vec::new();
    let mut start = 0;
    for (at, _) in text.match_indices('\'') {
        if text[at + 1..].starts_with([',', ';', ')', ']']) {
            pieces.push(&text[start..at]);
            start = at + 1;
        }
    }
    pieces.push(&text[start..]);
    pieces
}

/// `raw` without the punctuation after it. A closing bracket stays when its opener is inside.
fn without_punctuation(raw: &str) -> &str {
    let count = |bracket| raw.bytes().filter(|byte| *byte == bracket).count();
    let (round_open, square_open) = (count(b'('), count(b'['));
    let (mut round_close, mut square_close) = (count(b')'), count(b']'));
    let mut link = raw;
    while let Some(last) = link.chars().last() {
        let ends = match last {
            ',' | '.' | ';' | ':' | '!' | '?' | '\'' => true,
            ')' if round_close > round_open => {
                round_close -= 1;
                true
            }
            ']' if square_close > square_open => {
                square_close -= 1;
                true
            }
            _ => false,
        };
        if !ends {
            break;
        }
        link = &link[..link.len() - last.len_utf8()];
    }
    link
}

/// The links of a field's text and its lines without one, worked out again only when the text
/// changes.
#[derive(Default)]
pub struct Kept(RefCell<Option<(String, Rc<Found>)>>);

/// What a text holds.
pub struct Found {
    pub urls: Vec<String>,
    pub lines_without_links: usize,
}

impl Kept {
    pub fn of(&self, text: &str) -> Rc<Found> {
        let mut kept = self.0.borrow_mut();
        if let Some((_, found)) = kept.as_ref().filter(|(read, _)| read == text) {
            return found.clone();
        }
        let found = Rc::new(Found {
            urls: extract(text),
            lines_without_links: lines_without_links(text),
        });
        *kept = Some((text.to_owned(), found.clone()));
        found
    }
}

/// Lines with text but no link, which the Add dialog leaves out.
fn lines_without_links(text: &str) -> usize {
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
    fn a_huge_paste_is_read_in_one_pass() {
        const SIZE: usize = 1_000_000;
        let brackets = format!("https://example.com/a{}", ")".repeat(SIZE));
        assert_eq!(extract(&brackets), ["https://example.com/a"]);
        let squares = format!("https://example.com/a{}", "]".repeat(SIZE));
        assert_eq!(extract(&squares), ["https://example.com/a"]);
        let quoted: Vec<String> = (0..SIZE / 10)
            .map(|id| format!("'https://example.com/{id}',"))
            .collect();
        let quoted = quoted.concat();
        assert_eq!(extract(&quoted).len(), SIZE / 10);
    }

    #[test]
    fn a_text_is_read_again_only_when_it_changes() {
        let kept = Kept::default();
        let first = kept.of("https://example.com/a\nnotes");
        assert_eq!(first.urls, ["https://example.com/a"]);
        assert_eq!(first.lines_without_links, 1);
        assert!(Rc::ptr_eq(&first, &kept.of("https://example.com/a\nnotes")));
        assert_eq!(
            kept.of("https://example.com/b").urls,
            ["https://example.com/b"]
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
