//! The Public Suffix List: the names under which anyone may register a site of
//! their own (`com`, `co.uk`, `github.io`), so a name at or above one never
//! stands for one owner's sites (D39). Every question core asks of the list
//! goes through [`list`], so a copy the app refreshes on its own can take the
//! place of the built-in one there.

use psl::Psl;

/// The list core reads, both its ICANN and its private section: the copy built
/// into this release by the `psl` crate, which is published again with every
/// change to the list.
fn list() -> impl Psl {
    psl::List
}

/// The registrable domain `name` is or is under (`example.com` for
/// `wiki.corp.example.com`), or `None` when `name` is a public suffix or above
/// one, which no single owner holds. `name` is a host name as the URL parser
/// writes it: lower case, in ASCII form, without a trailing dot. A last label
/// the list does not know counts as a suffix of its own, as the list's
/// implicit `*` rule says, so `corp.mycompany` is registrable and
/// `mycompany` is not.
pub(crate) fn registrable_domain(name: &str) -> Option<&str> {
    let domain = list().domain(name.as_bytes())?;
    std::str::from_utf8(domain.as_bytes()).ok()
}

/// Whether `name` (written as for [`registrable_domain`]) is itself on the
/// list (`com`, `ai`, `xn--p1ai`, `co.uk`, `github.io`), so public DNS may
/// serve it. A single label the list does not know (`nas`) is not.
pub(crate) fn is_public_suffix(name: &str) -> bool {
    list()
        .suffix(name.as_bytes())
        .is_some_and(|suffix| suffix.is_known() && suffix.as_bytes() == name.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_knows_both_its_sections_and_its_wildcards() {
        for suffix in ["com", "ai", "xn--p1ai", "co.uk", "github.io", "anything.ck"] {
            assert!(is_public_suffix(suffix), "{suffix}");
            assert_eq!(registrable_domain(suffix), None, "{suffix}");
        }
        for name in ["nas", "example.com", "team.github.io", "corp.mycompany"] {
            assert!(!is_public_suffix(name), "{name}");
        }
        assert_eq!(
            registrable_domain("wiki.corp.example.com"),
            Some("example.com")
        );
        assert_eq!(registrable_domain("team.github.io"), Some("team.github.io"));
        assert_eq!(registrable_domain("corp.mycompany"), Some("corp.mycompany"));
        assert_eq!(registrable_domain("mycompany"), None);
    }
}
