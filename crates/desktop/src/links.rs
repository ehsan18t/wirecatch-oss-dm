//! Links in pasted or typed text.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::LazyLock;

use regex::Regex;
use url::Url;
use wirecatch_engine::url::{FTP_SCHEMES, WEB_SCHEMES};
use wirecatch_shell::launch::{one_link, without_punctuation};

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

/// The links of a field's text and its lines without one, worked out again only when the text
/// changes.
#[derive(Default)]
pub struct Kept(RefCell<Option<(String, Rc<Found>)>>);

/// What a text holds.
pub struct Found {
    pub urls: Vec<String>,
    pub lines_without_links: usize,
    /// Lines with words after a link: the link may have held them, such as a raw space in its name.
    pub lines_with_text_after: usize,
    /// The whole text as one link, for a field that takes one.
    pub whole: Option<String>,
}

impl Kept {
    pub fn of(&self, text: &str) -> Rc<Found> {
        let mut kept = self.0.borrow_mut();
        if let Some((_, found)) = kept.as_ref().filter(|(read, _)| read == text) {
            return found.clone();
        }
        let urls = extract(text);
        let found = Rc::new(Found {
            whole: if urls.len() > 1 { None } else { one_link(text) },
            urls,
            lines_without_links: lines_without_links(text),
            lines_with_text_after: text
                .lines()
                .filter(|line| has_text_after_link(line))
                .count(),
        });
        *kept = Some((text.to_owned(), found.clone()));
        found
    }
}

/// What a copied text gives the Add dialog: all of it when it holds a link, so the dialog can say
/// what it left out.
pub fn pasted(text: &str) -> Vec<String> {
    if extract(text).is_empty() {
        Vec::new()
    } else {
        vec![text.to_owned()]
    }
}

/// `text` with each of `incoming` after it that brings a link it does not hold yet.
pub fn joined(text: &str, incoming: &[String]) -> String {
    let mut known: HashSet<String> = extract(text).into_iter().collect();
    let mut all = text.to_owned();
    for other in incoming {
        let brings_one = extract(other)
            .into_iter()
            .filter(|url| known.insert(url.clone()))
            .count()
            > 0;
        if brings_one {
            if !all.is_empty() {
                all.push('\n');
            }
            all.push_str(other);
        }
    }
    all
}

fn has_text_after_link(text: &str) -> bool {
    let Some(link) = LINK.as_ref() else {
        return false;
    };
    let matches: Vec<_> = link.find_iter(text).collect();
    matches.iter().enumerate().any(|(place, found)| {
        let until = matches
            .get(place + 1)
            .map_or(text.len(), regex::Match::start);
        text[found.end()..until].chars().any(char::is_alphanumeric)
    })
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
    fn words_after_a_link_are_counted_but_not_a_link_alone() {
        let kept = Kept::default();
        let count = |text| kept.of(text).lines_with_text_after;
        assert_eq!(count("https://example.com/a b.zip"), 1);
        assert_eq!(count("https://example.com/a b https://example.com/c"), 1);
        assert_eq!(
            count("https://example.com/a https://example.com/c\nnotes"),
            0
        );
        assert_eq!(count("See https://example.com/a."), 0);
        assert_eq!(count("\"https://example.com/a.zip\""), 0);
        assert_eq!(
            count("[\"https://example.com/a\", \"https://example.com/b\"]"),
            0
        );
    }

    #[test]
    fn a_field_for_one_link_reads_its_whole_text() {
        let whole = Kept::default()
            .of("https://example.com/a b.zip")
            .whole
            .clone();
        assert_eq!(whole.as_deref(), Some("https://example.com/a%20b.zip"));
        let two = Kept::default().of("https://example.com/a https://example.com/b");
        assert_eq!(two.whole, None);
        assert_eq!(two.urls.len(), 2);
    }

    #[test]
    fn a_copied_text_reaches_the_dialog_whole_when_it_holds_a_link() {
        assert_eq!(
            pasted("https://example.com/a b.zip"),
            ["https://example.com/a b.zip"]
        );
        assert_eq!(pasted("no link here"), Vec::<String>::new());
    }

    #[test]
    fn joining_adds_only_what_brings_a_new_link_and_keeps_the_text() {
        let text = "https://example.com/a b.zip";
        let incoming = [
            "https://example.com/a".to_owned(),
            "https://example.com/c d".to_owned(),
        ];
        assert_eq!(
            joined(text, &incoming),
            "https://example.com/a b.zip\nhttps://example.com/c d"
        );
        assert_eq!(joined("", &incoming[..1]), "https://example.com/a");
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
        let many: Vec<String> = (0..SIZE / 10)
            .map(|id| format!("https://example.com/{id}"))
            .collect();
        assert_eq!(extract(&joined(&many.join("\n"), &many)).len(), SIZE / 10);
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
