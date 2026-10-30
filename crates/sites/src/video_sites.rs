//! The sites whose pages show videos (D51): a link to one is read for its video when it is added.
//! The list is `packages/protocol/src/video-sites.json`, which the extension reads too.

use std::sync::LazyLock;

use regex::Regex;
use serde::Deserialize;
use url::Url;
use wirecatch_protocol::extension::normalize_site;

const LIST: &str = include_str!("../../../packages/protocol/src/video-sites.json");

#[derive(Deserialize)]
struct Listed {
    name: String,
    hosts: Vec<String>,
    path: Option<String>,
    #[serde(default)]
    ignore_case: bool,
}

struct Site {
    name: String,
    hosts: Vec<String>,
    /// Matched against the path; `None` takes any path but the home page.
    path: Option<Regex>,
}

/// The list as built in; its test reads every entry, so a bad one never ships.
static SITES: LazyLock<Vec<Site>> = LazyLock::new(|| {
    let listed: Vec<Listed> = serde_json::from_str(LIST).unwrap_or_else(|e| {
        tracing::error!("the video site list does not read: {e}");
        Vec::new()
    });
    listed
        .into_iter()
        .filter_map(|site| {
            let path = match site.path {
                None => None,
                Some(pattern) => {
                    let pattern = if site.ignore_case {
                        format!("(?i){pattern}")
                    } else {
                        pattern
                    };
                    match Regex::new(&pattern) {
                        Ok(path) => Some(path),
                        Err(e) => {
                            tracing::error!("the video site {} has a bad path: {e}", site.name);
                            return None;
                        }
                    }
                }
            };
            Some(Site {
                name: site.name,
                hosts: site.hosts,
                path,
            })
        })
        .collect()
});

/// The site's name when `url` is an http or https page that shows a video: on a listed site, or
/// on one of the `added` sites (any page but the home page), named then by the site as added.
#[must_use]
pub fn video_site_for(url: &str, added: &[String]) -> Option<String> {
    let page = Url::parse(url)
        .ok()
        .filter(|page| matches!(page.scheme(), "http" | "https"))?;
    let host = page.host_str()?.to_ascii_lowercase();
    let path = page.path();
    let listed = SITES.iter().find(|site| {
        site.hosts.iter().any(|on| on_site(&host, on))
            && site
                .path
                .as_ref()
                .map_or(path.len() > 1, |pattern| pattern.is_match(path))
    });
    if let Some(site) = listed {
        return Some(site.name.clone());
    }
    if path.len() <= 1 {
        return None;
    }
    added
        .iter()
        .filter_map(|site| normalize_site(site))
        .find(|site| on_site(&host, site))
}

/// Whether `host` is `site` or one of its subdomains.
fn on_site(host: &str, site: &str) -> bool {
    host.strip_suffix(site)
        .is_some_and(|rest| rest.is_empty() || rest.ends_with('.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize)]
    struct Cases {
        pages: Vec<Page>,
        not_pages: Vec<String>,
    }

    #[derive(Deserialize)]
    struct Page {
        url: String,
        name: String,
    }

    fn cases() -> Cases {
        serde_json::from_str(include_str!(
            "../../../packages/protocol/src/video-sites.cases.json"
        ))
        .unwrap()
    }

    #[test]
    fn every_listed_site_reads() {
        let listed: Vec<Listed> = serde_json::from_str(LIST).unwrap();
        assert_eq!(SITES.len(), listed.len());
    }

    #[test]
    fn video_pages_are_named_by_their_site() {
        for page in cases().pages {
            assert_eq!(
                video_site_for(&page.url, &[]).as_deref(),
                Some(page.name.as_str()),
                "{}",
                page.url
            );
        }
    }

    #[test]
    fn other_pages_are_not_taken() {
        for url in cases().not_pages {
            assert_eq!(video_site_for(&url, &[]), None, "{url}");
        }
    }

    #[test]
    fn a_site_the_person_added_takes_any_page_but_its_home_page() {
        let added = vec!["example.org".to_owned()];
        assert_eq!(
            video_site_for("https://videos.example.org/talks/42", &added).as_deref(),
            Some("example.org")
        );
        assert_eq!(video_site_for("https://example.org/", &added), None);
        assert_eq!(
            video_site_for("https://badexample.org/talks/42", &added),
            None
        );
        let listed = vec!["youtu.be".to_owned()];
        assert_eq!(
            video_site_for("https://youtu.be/abc", &listed).as_deref(),
            Some("YouTube")
        );
    }
}
