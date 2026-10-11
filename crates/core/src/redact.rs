//! Links as people see them: a password in a link (`https://user:secret@host/`)
//! is never shown or copied. The manager keeps and uses the real link.

/// `url` with the password in it, if any, replaced by `***`
/// (`https://ada:***@example.com/file`). A link with a user name but no
/// password hides the name the same way, since some sites put a token there.
/// Anything else is returned as it is.
pub(crate) fn url_for_display(url: &str) -> String {
    let Some(scheme_end) = url.find("://") else {
        return url.to_string();
    };
    let start = scheme_end + 3;
    let rest = &url[start..];
    let authority = &rest[..rest.find(['/', '?', '#']).unwrap_or(rest.len())];
    let Some(at) = authority.rfind('@') else {
        return url.to_string();
    };
    let masked = match authority[..at].split_once(':') {
        Some((user, _)) => format!("{user}:***"),
        None => "***".to_string(),
    };
    format!("{}{masked}{}", &url[..start], &rest[at..])
}

/// `text` (an error message, say) with the password of every link in it
/// hidden the way [`url_for_display`] does.
pub(crate) fn text_for_display(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(found) = rest.find("://") {
        let start = found + 3;
        out.push_str(&rest[..start]);
        let after = &rest[start..];
        let end = after
            .find(|c: char| {
                c.is_whitespace() || matches!(c, '/' | '?' | '#' | '"' | '\'' | '<' | '>')
            })
            .unwrap_or(after.len());
        let authority = &after[..end];
        match authority.rfind('@') {
            Some(at) => {
                match authority[..at].split_once(':') {
                    Some((user, _)) => {
                        out.push_str(user);
                        out.push_str(":***");
                    }
                    None => out.push_str("***"),
                }
                out.push_str(&authority[at..]);
            }
            None => out.push_str(authority),
        }
        rest = &after[end..];
    }
    out.push_str(rest);
    out
}

/// [`url_for_display`] for an optional link.
pub(crate) fn optional_url_for_display(url: Option<&str>) -> Option<String> {
    url.map(url_for_display)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passwords_in_links_are_hidden() {
        for (url, shown) in [
            (
                "https://ada:secret@example.com/file.zip",
                "https://ada:***@example.com/file.zip",
            ),
            (
                "ftp://ada:p%40ss:word@files.example.com:21/a?b=c@d",
                "ftp://ada:***@files.example.com:21/a?b=c@d",
            ),
            (
                "https://token123@example.com/x",
                "https://***@example.com/x",
            ),
            ("https://ada:@example.com", "https://ada:***@example.com"),
            (
                "https://example.com/a@b/c?mail=x@y",
                "https://example.com/a@b/c?mail=x@y",
            ),
            ("https://example.com", "https://example.com"),
            ("not a link", "not a link"),
        ] {
            assert_eq!(url_for_display(url), shown, "{url}");
        }
        assert_eq!(optional_url_for_display(None), None);
    }

    #[test]
    fn passwords_in_links_inside_messages_are_hidden() {
        assert_eq!(
            text_for_display(
                "server answered 403 for https://ada:secret@example.com/f and ftp://u:p@h:21."
            ),
            "server answered 403 for https://ada:***@example.com/f and ftp://u:***@h:21."
        );
        assert_eq!(
            text_for_display("mail ada@example.com about http://example.com/x"),
            "mail ada@example.com about http://example.com/x"
        );
    }
}
