//! Links in pasted or typed text.

use std::sync::LazyLock;

use regex::Regex;
use url::Url;

/// An http, https or ftp link, up to the first character no link holds.
static LINK: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r#"(?i)\b(?:https?|ftps?)://[^\s<>"'`]+"#).ok());

/// Every link in `text`, each once, in order. Punctuation that ends a sentence after a link is
/// not part of it.
pub fn extract(text: &str) -> Vec<String> {
    let Some(link) = LINK.as_ref() else {
        return Vec::new();
    };
    let mut found: Vec<String> = Vec::new();
    for raw in link.find_iter(text) {
        let cleaned = raw
            .as_str()
            .trim_end_matches([')', ',', '.', ';', ':', '!', '?', ']']);
        if let Ok(url) = Url::parse(cleaned) {
            let url = String::from(url);
            if !found.contains(&url) {
                found.push(url);
            }
        }
    }
    found
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
}
