//! Which queue rules a new download (D72.3): the first kind of rule that applies, in the order site, file name, file type, category, protocol; within a kind, the most specific site, else the queue highest in the sidebar.

use std::collections::BTreeSet;

use regex::{Regex, RegexBuilder};
use url::Url;
use wirecatch_protocol::QueueId;
use wirecatch_protocol::app::Category;
use wirecatch_protocol::extension::normalize_file_type;
use wirecatch_protocol::rules::{
    LinkProtocol, MAX_RULE_CHARS, MAX_RULES_PER_KIND, NameSyntax, QueueRules, Rule,
};

use crate::sites::SitePattern;

/// Bytes a compiled file name expression may take, so a huge repetition cannot use up memory.
const REGEX_SIZE_LIMIT: usize = 1 << 20;

/// A new download as the rules read it.
#[derive(Debug, Clone, Copy)]
pub struct Link<'a> {
    pub url: &'a Url,
    /// The name known when the download is added: the browser's for a captured file, else the link's (D72.3).
    pub file_name: &'a str,
    pub category: Category,
}

/// An auto queue's rules, in sidebar order. `K` names the queue: its id, or for Try a link one that also names a queue not made yet.
#[derive(Debug, Clone, Copy)]
pub struct RuleQueue<'a, K = QueueId> {
    pub id: K,
    pub rules: &'a QueueRules,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found<K = QueueId> {
    pub queue: K,
    pub rule: Rule,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Matched<K = QueueId> {
    /// The queue whose rule takes the download, among the queues that are on.
    pub matched: Option<Found<K>>,
    /// A queue that is off and would have matched it from `matched`, for Try a link (D72.5).
    pub switched_off: Option<Found<K>>,
}

/// Where `link` goes among `queues`; `is_local` says whether a host is on the person's own network (D34).
#[must_use]
pub fn find<K: Copy + PartialEq>(
    queues: &[RuleQueue<'_, K>],
    link: &Link<'_>,
    is_local: &dyn Fn(&Url) -> bool,
) -> Matched<K> {
    let on: Vec<RuleQueue<'_, K>> = queues.iter().copied().filter(|q| q.rules.enabled).collect();
    let matched = first_rule(&on, link, is_local);
    let any = first_rule(queues, link, is_local);
    let switched_off = any.filter(|found| {
        queues
            .iter()
            .any(|q| q.id == found.queue && !q.rules.enabled)
    });
    Matched {
        matched,
        switched_off,
    }
}

fn first_rule<K: Copy>(
    queues: &[RuleQueue<'_, K>],
    link: &Link<'_>,
    is_local: &dyn Fn(&Url) -> bool,
) -> Option<Found<K>> {
    best_site(queues, link.url, is_local)
        .or_else(|| first(queues, |c| file_name_rule(c, link.file_name)))
        .or_else(|| first(queues, |c| file_type_rule(c, link.file_name)))
        .or_else(|| {
            first(queues, |c| {
                c.categories
                    .contains(&link.category)
                    .then_some(Rule::Category(link.category))
            })
        })
        .or_else(|| {
            let protocol = protocol_of(link.url)?;
            first(queues, |c| {
                c.protocols
                    .contains(&protocol)
                    .then_some(Rule::Protocol(protocol))
            })
        })
}

fn first<K: Copy>(
    queues: &[RuleQueue<'_, K>],
    rule: impl Fn(&QueueRules) -> Option<Rule>,
) -> Option<Found<K>> {
    queues
        .iter()
        .find_map(|q| rule(q.rules).map(|rule| Found { queue: q.id, rule }))
}

/// The most specific site rule that covers the link; between equally specific ones, the queue higher in the sidebar.
fn best_site<K: Copy>(
    queues: &[RuleQueue<'_, K>],
    url: &Url,
    is_local: &dyn Fn(&Url) -> bool,
) -> Option<Found<K>> {
    let mut best: Option<(u16, Found<K>)> = None;
    for q in queues {
        for written in &q.rules.sites {
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
                    rule: Rule::Site(written.clone()),
                };
                best = Some((specificity, found));
            }
        }
    }
    best.map(|(_, found)| found)
}

fn file_name_rule(rules: &QueueRules, file_name: &str) -> Option<Rule> {
    rules
        .file_names
        .iter()
        .find(|written| {
            name_matcher(written, rules.file_name_syntax).is_ok_and(|m| m.is_match(file_name))
        })
        .map(|written| Rule::FileName(written.clone()))
}

/// The longest extension rule the name ends with, so `tar.gz` wins over `gz`.
fn file_type_rule(rules: &QueueRules, file_name: &str) -> Option<Rule> {
    let name = file_name.to_lowercase();
    rules
        .file_types
        .iter()
        .filter(|ext| {
            name.strip_suffix(ext.as_str())
                .is_some_and(|stem| stem.len() > 1 && stem.ends_with('.'))
        })
        .max_by_key(|ext| ext.len())
        .map(|ext| Rule::FileType(ext.clone()))
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

/// `rules` as they are kept: each rule trimmed, empty and repeated ones dropped, file types in their stored form; or why they cannot be kept, in words for the person.
pub fn checked(mut rules: QueueRules) -> Result<QueueRules, String> {
    rules.sites = cleaned(&rules.sites, "sites", |site| {
        SitePattern::read(site)
            .map(|_| site.to_string())
            .ok_or_else(|| format!("\"{site}\" is not a site, an address, a range or <local>."))
    })?;
    let syntax = rules.file_name_syntax;
    rules.file_names = cleaned(&rules.file_names, "file names", |name| {
        name_matcher(name, syntax)
            .map(|_| name.to_string())
            .map_err(|_| format!("\"{name}\" is not a regular expression Wirecatch can use."))
    })?;
    rules.file_types = cleaned(&rules.file_types, "file types", |kind| {
        normalize_file_type(kind).ok_or_else(|| format!("\"{kind}\" is not a file type."))
    })?;
    rules.categories = dedupe(rules.categories);
    rules.protocols = dedupe(rules.protocols);
    Ok(rules)
}

fn cleaned(
    written: &[String],
    what: &str,
    read: impl Fn(&str) -> Result<String, String>,
) -> Result<Vec<String>, String> {
    let mut seen = BTreeSet::new();
    let mut kept = Vec::new();
    for rule in written {
        let rule = rule.trim();
        if rule.is_empty() {
            continue;
        }
        if rule.chars().count() > MAX_RULE_CHARS {
            return Err(format!(
                "A rule can be at most {MAX_RULE_CHARS} characters; one of the {what} is longer."
            ));
        }
        let read = read(rule)?;
        if seen.insert(read.to_lowercase()) {
            kept.push(read);
        }
    }
    if kept.len() > MAX_RULES_PER_KIND {
        return Err(format!(
            "A queue can rule at most {MAX_RULES_PER_KIND} {what}."
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

    fn auto(change: impl FnOnce(&mut QueueRules)) -> QueueRules {
        let mut rules = QueueRules {
            enabled: true,
            ..QueueRules::default()
        };
        change(&mut rules);
        rules
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(ToString::to_string).collect()
    }

    fn matched(
        queues: &[(i64, &QueueRules)],
        link: &str,
        name: &str,
        category: Category,
    ) -> Option<Found> {
        match_in(queues, link, name, category).matched
    }

    fn match_in(
        queues: &[(i64, &QueueRules)],
        link: &str,
        name: &str,
        category: Category,
    ) -> Matched {
        let queues: Vec<RuleQueue<'_>> = queues
            .iter()
            .map(|(id, rules)| RuleQueue {
                id: QueueId(*id),
                rules,
            })
            .collect();
        let url = url(link);
        find(
            &queues,
            &Link {
                url: &url,
                file_name: name,
                category,
            },
            &|u| u.host_str() == Some("nas"),
        )
    }

    fn found(queue: i64, rule: Rule) -> Found {
        Found {
            queue: QueueId(queue),
            rule,
        }
    }

    #[test]
    fn a_site_rule_comes_before_every_other_kind() {
        let videos = auto(|c| c.sites = strings(&["youtube.com"]));
        let isos = auto(|c| c.file_types = strings(&["mp4"]));
        assert_eq!(
            matched(
                &[(3, &isos), (2, &videos)],
                "https://www.youtube.com/v",
                "talk.mp4",
                Category::Video
            ),
            Some(found(2, Rule::Site("youtube.com".into())))
        );
    }

    #[test]
    fn the_most_specific_site_wins_whatever_the_order() {
        let broad = auto(|c| c.sites = strings(&["example.com"]));
        let narrow = auto(|c| c.sites = strings(&["cdn.example.com"]));
        assert_eq!(
            matched(
                &[(2, &broad), (3, &narrow)],
                "https://cdn.example.com/a",
                "a.bin",
                Category::Other
            ),
            Some(found(3, Rule::Site("cdn.example.com".into())))
        );
    }

    #[test]
    fn between_equal_rules_the_queue_higher_in_the_sidebar_wins() {
        let first = auto(|c| c.categories = vec![Category::Archive]);
        let second = auto(|c| c.categories = vec![Category::Archive]);
        assert_eq!(
            matched(
                &[(4, &first), (2, &second)],
                "https://a.example/x.zip",
                "x.zip",
                Category::Archive
            ),
            Some(found(4, Rule::Category(Category::Archive)))
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
        let at = |name: &str| matched(&queues, "ftp://files.example/x", name, Category::Document);
        assert_eq!(
            at("Report-2026.PDF"),
            Some(found(2, Rule::FileName("report-*.pdf".into())))
        );
        assert_eq!(
            at("notes.pdf"),
            Some(found(3, Rule::FileType("pdf".into())))
        );
        assert_eq!(
            at("notes.docx"),
            Some(found(4, Rule::Category(Category::Document)))
        );
        let other = matched(&queues, "ftps://files.example/x", "a.bin", Category::Other);
        assert_eq!(other, Some(found(5, Rule::Protocol(LinkProtocol::Ftp))));
    }

    #[test]
    fn file_names_match_whole_by_wildcard_or_anywhere_by_expression() {
        let wild = auto(|c| c.file_names = strings(&["*.iso", "disk?.img"]));
        let expr = auto(|c| {
            c.file_name_syntax = NameSyntax::Regex;
            c.file_names = strings(&[r"\bs\d{2}e\d{2}\b"]);
        });
        let at = |rules: &QueueRules, name: &str| {
            matched(&[(2, rules)], "https://a.example/", name, Category::Other)
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
            matched(
                &[(2, &types)],
                "https://a.example/",
                "src.TAR.GZ",
                Category::Archive
            ),
            Some(found(2, Rule::FileType("tar.gz".into())))
        );
        assert!(matched(&[(2, &types)], "https://a.example/", "gz", Category::Other).is_none());
    }

    #[test]
    fn a_queue_that_is_off_rules_nothing_and_says_it_would_have() {
        let off = auto(|c| {
            c.enabled = false;
            c.sites = strings(&["example.com"]);
        });
        let on = auto(|c| c.protocols = vec![LinkProtocol::Https]);
        let result = match_in(
            &[(2, &off), (3, &on)],
            "https://example.com/a",
            "a.bin",
            Category::Other,
        );
        assert_eq!(
            result.matched,
            Some(found(3, Rule::Protocol(LinkProtocol::Https)))
        );
        assert_eq!(
            result.switched_off,
            Some(found(2, Rule::Site("example.com".into())))
        );
    }

    #[test]
    fn rules_are_kept_cleaned_and_refused_when_they_name_nothing() {
        let kept = checked(auto(|c| {
            c.sites = strings(&[" youtube.com ", "", "YouTube.com", "<local>"]);
            c.file_types = strings(&[".ISO", "*.tar.gz", "iso"]);
            c.categories = vec![Category::Video, Category::Video];
        }))
        .unwrap();
        assert_eq!(kept.sites, strings(&["youtube.com", "<local>"]));
        assert_eq!(kept.file_types, strings(&["iso", "tar.gz"]));
        assert_eq!(kept.categories, vec![Category::Video]);

        let refused = |change: fn(&mut QueueRules)| checked(auto(change)).unwrap_err();
        assert!(refused(|c| c.sites = strings(&["10.*.1"])).contains("10.*.1"));
        assert!(
            refused(|c| {
                c.file_name_syntax = NameSyntax::Regex;
                c.file_names = strings(&["(unclosed"]);
            })
            .contains("regular expression")
        );
        assert!(
            refused(|c| c.file_names = vec!["x".repeat(MAX_RULE_CHARS + 1)]).contains("at most")
        );
        assert!(
            refused(|c| c.file_types = (0..=MAX_RULES_PER_KIND).map(|i| format!("e{i}")).collect())
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
