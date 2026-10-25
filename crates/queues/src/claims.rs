//! Which queue claims a new download (D72.3): the first kind of claim that applies, in the order site, file name, file type, category, protocol; within a kind, the most specific site, else the queue highest in the sidebar.

use std::collections::BTreeSet;

use regex::{Regex, RegexBuilder};
use url::Url;
use wirecatch_protocol::QueueId;
use wirecatch_protocol::app::Category;
use wirecatch_protocol::claims::{
    Claim, LinkProtocol, MAX_CLAIM_CHARS, MAX_CLAIMS_PER_KIND, NameSyntax, QueueClaims,
};
use wirecatch_protocol::extension::normalize_file_type;

use crate::sites::SitePattern;

/// Bytes a compiled file name expression may take, so a huge repetition cannot use up memory.
const REGEX_SIZE_LIMIT: usize = 1 << 20;

/// A new download as the claims read it.
#[derive(Debug, Clone, Copy)]
pub struct Link<'a> {
    pub url: &'a Url,
    /// The name known when the download is added: the browser's for a captured file, else the link's (D72.3).
    pub file_name: &'a str,
    pub category: Category,
}

/// An auto queue's claims, in sidebar order. `K` names the queue: its id, or for Try a link one that also names a queue not made yet.
#[derive(Debug, Clone, Copy)]
pub struct ClaimingQueue<'a, K = QueueId> {
    pub id: K,
    pub claims: &'a QueueClaims,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found<K = QueueId> {
    pub queue: K,
    pub claim: Claim,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claimed<K = QueueId> {
    /// The queue whose claim takes the download, among the queues that are on.
    pub taken: Option<Found<K>>,
    /// A queue that is off and would have taken it from `taken`, for Try a link (D72.5).
    pub switched_off: Option<Found<K>>,
}

/// Where `link` goes among `queues`; `is_local` says whether a host is on the person's own network (D34).
#[must_use]
pub fn claim<K: Copy + PartialEq>(
    queues: &[ClaimingQueue<'_, K>],
    link: &Link<'_>,
    is_local: &dyn Fn(&Url) -> bool,
) -> Claimed<K> {
    let on: Vec<ClaimingQueue<'_, K>> = queues
        .iter()
        .copied()
        .filter(|q| q.claims.enabled)
        .collect();
    let taken = first_claim(&on, link, is_local);
    let any = first_claim(queues, link, is_local);
    let switched_off = any.filter(|found| {
        queues
            .iter()
            .any(|q| q.id == found.queue && !q.claims.enabled)
    });
    Claimed {
        taken,
        switched_off,
    }
}

fn first_claim<K: Copy>(
    queues: &[ClaimingQueue<'_, K>],
    link: &Link<'_>,
    is_local: &dyn Fn(&Url) -> bool,
) -> Option<Found<K>> {
    best_site(queues, link.url, is_local)
        .or_else(|| first(queues, |c| file_name_claim(c, link.file_name)))
        .or_else(|| first(queues, |c| file_type_claim(c, link.file_name)))
        .or_else(|| {
            first(queues, |c| {
                c.categories
                    .contains(&link.category)
                    .then_some(Claim::Category(link.category))
            })
        })
        .or_else(|| {
            let protocol = protocol_of(link.url)?;
            first(queues, |c| {
                c.protocols
                    .contains(&protocol)
                    .then_some(Claim::Protocol(protocol))
            })
        })
}

fn first<K: Copy>(
    queues: &[ClaimingQueue<'_, K>],
    claim: impl Fn(&QueueClaims) -> Option<Claim>,
) -> Option<Found<K>> {
    queues
        .iter()
        .find_map(|q| claim(q.claims).map(|claim| Found { queue: q.id, claim }))
}

/// The most specific site claim that covers the link; between equally specific ones, the queue higher in the sidebar.
fn best_site<K: Copy>(
    queues: &[ClaimingQueue<'_, K>],
    url: &Url,
    is_local: &dyn Fn(&Url) -> bool,
) -> Option<Found<K>> {
    let mut best: Option<(u16, Found<K>)> = None;
    for q in queues {
        for written in &q.claims.sites {
            let Some(pattern) = SitePattern::read(written) else {
                continue;
            };
            if !pattern.covers(url, is_local) {
                continue;
            }
            let specificity = pattern.specificity();
            if best.as_ref().is_none_or(|(b, _)| specificity > *b) {
                let found = Found {
                    queue: q.id,
                    claim: Claim::Site(written.clone()),
                };
                best = Some((specificity, found));
            }
        }
    }
    best.map(|(_, found)| found)
}

fn file_name_claim(claims: &QueueClaims, file_name: &str) -> Option<Claim> {
    claims
        .file_names
        .iter()
        .find(|written| {
            name_matcher(written, claims.file_name_syntax).is_ok_and(|m| m.is_match(file_name))
        })
        .map(|written| Claim::FileName(written.clone()))
}

/// The longest extension claim the name ends with, so `tar.gz` wins over `gz`.
fn file_type_claim(claims: &QueueClaims, file_name: &str) -> Option<Claim> {
    let name = file_name.to_lowercase();
    claims
        .file_types
        .iter()
        .filter(|ext| {
            name.strip_suffix(ext.as_str())
                .is_some_and(|stem| stem.len() > 1 && stem.ends_with('.'))
        })
        .max_by_key(|ext| ext.len())
        .map(|ext| Claim::FileType(ext.clone()))
}

fn protocol_of(url: &Url) -> Option<LinkProtocol> {
    match url.scheme() {
        "http" => Some(LinkProtocol::Http),
        "https" => Some(LinkProtocol::Https),
        "ftp" | "ftps" | "ftpes" => Some(LinkProtocol::Ftp),
        _ => None,
    }
}

fn name_matcher(written: &str, syntax: NameSyntax) -> Result<Regex, regex::Error> {
    let pattern = match syntax {
        NameSyntax::Regex => written.to_string(),
        NameSyntax::Wildcards => {
            let mut pattern = String::from("^");
            for c in written.chars() {
                match c {
                    '*' => pattern.push_str(".*"),
                    '?' => pattern.push('.'),
                    c => pattern.push_str(&regex::escape(&c.to_string())),
                }
            }
            pattern.push('$');
            pattern
        }
    };
    RegexBuilder::new(&pattern)
        .case_insensitive(true)
        .size_limit(REGEX_SIZE_LIMIT)
        .build()
}

/// `claims` as they are kept: each claim trimmed, empty and repeated ones dropped, file types in their stored form; or why they cannot be kept, in words for the person.
pub fn checked(mut claims: QueueClaims) -> Result<QueueClaims, String> {
    claims.sites = cleaned(&claims.sites, "sites", |site| {
        SitePattern::read(site)
            .map(|_| site.to_string())
            .ok_or_else(|| format!("\"{site}\" is not a site, an address, a range or <local>."))
    })?;
    let syntax = claims.file_name_syntax;
    claims.file_names = cleaned(&claims.file_names, "file names", |name| {
        name_matcher(name, syntax)
            .map(|_| name.to_string())
            .map_err(|_| format!("\"{name}\" is not a regular expression Wirecatch can use."))
    })?;
    claims.file_types = cleaned(&claims.file_types, "file types", |kind| {
        normalize_file_type(kind).ok_or_else(|| format!("\"{kind}\" is not a file type."))
    })?;
    claims.categories = dedupe(claims.categories);
    claims.protocols = dedupe(claims.protocols);
    Ok(claims)
}

fn cleaned(
    written: &[String],
    what: &str,
    read: impl Fn(&str) -> Result<String, String>,
) -> Result<Vec<String>, String> {
    let mut seen = BTreeSet::new();
    let mut kept = Vec::new();
    for claim in written {
        let claim = claim.trim();
        if claim.is_empty() {
            continue;
        }
        if claim.chars().count() > MAX_CLAIM_CHARS {
            return Err(format!(
                "A claim can be at most {MAX_CLAIM_CHARS} characters; one of the {what} is longer."
            ));
        }
        let read = read(claim)?;
        if seen.insert(read.to_lowercase()) {
            kept.push(read);
        }
    }
    if kept.len() > MAX_CLAIMS_PER_KIND {
        return Err(format!(
            "A queue can claim at most {MAX_CLAIMS_PER_KIND} {what}."
        ));
    }
    Ok(kept)
}

fn dedupe<T: PartialEq>(items: Vec<T>) -> Vec<T> {
    let mut kept = Vec::with_capacity(items.len());
    for item in items {
        if !kept.contains(&item) {
            kept.push(item);
        }
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    fn auto(change: impl FnOnce(&mut QueueClaims)) -> QueueClaims {
        let mut claims = QueueClaims {
            enabled: true,
            ..QueueClaims::default()
        };
        change(&mut claims);
        claims
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(ToString::to_string).collect()
    }

    fn taken(
        queues: &[(i64, &QueueClaims)],
        link: &str,
        name: &str,
        category: Category,
    ) -> Option<Found> {
        claimed(queues, link, name, category).taken
    }

    fn claimed(
        queues: &[(i64, &QueueClaims)],
        link: &str,
        name: &str,
        category: Category,
    ) -> Claimed {
        let queues: Vec<ClaimingQueue<'_>> = queues
            .iter()
            .map(|(id, claims)| ClaimingQueue {
                id: QueueId(*id),
                claims,
            })
            .collect();
        let url = url(link);
        claim(
            &queues,
            &Link {
                url: &url,
                file_name: name,
                category,
            },
            &|u| u.host_str() == Some("nas"),
        )
    }

    fn found(queue: i64, claim: Claim) -> Found {
        Found {
            queue: QueueId(queue),
            claim,
        }
    }

    #[test]
    fn a_site_claim_comes_before_every_other_kind() {
        let videos = auto(|c| c.sites = strings(&["youtube.com"]));
        let isos = auto(|c| c.file_types = strings(&["mp4"]));
        assert_eq!(
            taken(
                &[(3, &isos), (2, &videos)],
                "https://www.youtube.com/v",
                "talk.mp4",
                Category::Video
            ),
            Some(found(2, Claim::Site("youtube.com".into())))
        );
    }

    #[test]
    fn the_most_specific_site_wins_whatever_the_order() {
        let broad = auto(|c| c.sites = strings(&["example.com"]));
        let narrow = auto(|c| c.sites = strings(&["cdn.example.com"]));
        assert_eq!(
            taken(
                &[(2, &broad), (3, &narrow)],
                "https://cdn.example.com/a",
                "a.bin",
                Category::Other
            ),
            Some(found(3, Claim::Site("cdn.example.com".into())))
        );
    }

    #[test]
    fn between_equal_claims_the_queue_higher_in_the_sidebar_wins() {
        let first = auto(|c| c.categories = vec![Category::Archive]);
        let second = auto(|c| c.categories = vec![Category::Archive]);
        assert_eq!(
            taken(
                &[(4, &first), (2, &second)],
                "https://a.example/x.zip",
                "x.zip",
                Category::Archive
            ),
            Some(found(4, Claim::Category(Category::Archive)))
        );
    }

    #[test]
    fn the_kinds_apply_in_order() {
        let by_name = auto(|c| c.file_names = strings(&["report-*.pdf"]));
        let by_type = auto(|c| c.file_types = strings(&["pdf"]));
        let by_category = auto(|c| c.categories = vec![Category::Document]);
        let by_protocol = auto(|c| c.protocols = vec![LinkProtocol::Ftp]);
        let queues = [
            (5, &by_protocol),
            (4, &by_category),
            (3, &by_type),
            (2, &by_name),
        ];
        let at = |name: &str| taken(&queues, "ftp://files.example/x", name, Category::Document);
        assert_eq!(
            at("Report-2026.PDF"),
            Some(found(2, Claim::FileName("report-*.pdf".into())))
        );
        assert_eq!(
            at("notes.pdf"),
            Some(found(3, Claim::FileType("pdf".into())))
        );
        assert_eq!(
            at("notes.docx"),
            Some(found(4, Claim::Category(Category::Document)))
        );
        let other = taken(&queues, "ftps://files.example/x", "a.bin", Category::Other);
        assert_eq!(other, Some(found(5, Claim::Protocol(LinkProtocol::Ftp))));
    }

    #[test]
    fn file_names_match_whole_by_wildcard_or_anywhere_by_expression() {
        let wild = auto(|c| c.file_names = strings(&["*.iso", "disk?.img"]));
        let expr = auto(|c| {
            c.file_name_syntax = NameSyntax::Regex;
            c.file_names = strings(&[r"\bs\d{2}e\d{2}\b"]);
        });
        let at = |claims: &QueueClaims, name: &str| {
            taken(&[(2, claims)], "https://a.example/", name, Category::Other)
        };
        assert!(at(&wild, "Debian.ISO").is_some());
        assert!(at(&wild, "disk1.img").is_some());
        assert!(at(&wild, "disk12.img").is_none());
        assert!(at(&wild, "debian.iso.part").is_none());
        assert!(at(&expr, "Show.S01E02.mkv").is_some());
        assert!(at(&expr, "Show.mkv").is_none());
    }

    #[test]
    fn the_longest_file_type_wins() {
        let types = auto(|c| c.file_types = strings(&["gz", "tar.gz"]));
        assert_eq!(
            taken(
                &[(2, &types)],
                "https://a.example/",
                "src.TAR.GZ",
                Category::Archive
            ),
            Some(found(2, Claim::FileType("tar.gz".into())))
        );
        assert!(taken(&[(2, &types)], "https://a.example/", "gz", Category::Other).is_none());
    }

    #[test]
    fn a_queue_that_is_off_claims_nothing_and_says_it_would_have() {
        let off = auto(|c| {
            c.enabled = false;
            c.sites = strings(&["example.com"]);
        });
        let on = auto(|c| c.protocols = vec![LinkProtocol::Https]);
        let result = claimed(
            &[(2, &off), (3, &on)],
            "https://example.com/a",
            "a.bin",
            Category::Other,
        );
        assert_eq!(
            result.taken,
            Some(found(3, Claim::Protocol(LinkProtocol::Https)))
        );
        assert_eq!(
            result.switched_off,
            Some(found(2, Claim::Site("example.com".into())))
        );
    }

    #[test]
    fn claims_are_kept_cleaned_and_refused_when_they_name_nothing() {
        let kept = checked(auto(|c| {
            c.sites = strings(&[" youtube.com ", "", "YouTube.com", "<local>"]);
            c.file_types = strings(&[".ISO", "*.tar.gz", "iso"]);
            c.categories = vec![Category::Video, Category::Video];
        }))
        .unwrap();
        assert_eq!(kept.sites, strings(&["youtube.com", "<local>"]));
        assert_eq!(kept.file_types, strings(&["iso", "tar.gz"]));
        assert_eq!(kept.categories, vec![Category::Video]);

        let refused = |change: fn(&mut QueueClaims)| checked(auto(change)).unwrap_err();
        assert!(refused(|c| c.sites = strings(&["10.*.1"])).contains("10.*.1"));
        assert!(
            refused(|c| {
                c.file_name_syntax = NameSyntax::Regex;
                c.file_names = strings(&["(unclosed"]);
            })
            .contains("regular expression")
        );
        assert!(
            refused(|c| c.file_names = vec!["x".repeat(MAX_CLAIM_CHARS + 1)]).contains("at most")
        );
        assert!(
            refused(|c| c.file_types = (0..=MAX_CLAIMS_PER_KIND).map(|i| format!("e{i}")).collect())
                .contains("at most")
        );
    }

    #[test]
    fn an_expression_too_large_to_build_is_refused() {
        let huge = checked(auto(|c| {
            c.file_name_syntax = NameSyntax::Regex;
            c.file_names = strings(&["(a{1000}){1000}"]);
        }));
        assert!(huge.is_err());
    }
}
