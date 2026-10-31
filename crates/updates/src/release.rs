//! GitHub's list of releases, and which of them is the update to offer.

use semver::Version;
use serde::Deserialize;

/// A release as GitHub's API lists it, with what updates read.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Release {
    pub tag_name: String,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub prerelease: bool,
    /// The release's page, for what is new and for a package the app cannot install itself.
    pub html_url: String,
    #[serde(default)]
    pub assets: Vec<Asset>,
}

/// A file attached to a release.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
    pub size: u64,
}

impl Release {
    /// The version its tag names (`v1.2.3` or `1.2.3`), if it names one.
    #[must_use]
    pub fn version(&self) -> Option<Version> {
        Version::parse(self.tag_name.trim().trim_start_matches('v')).ok()
    }

    /// The asset named `name`.
    #[must_use]
    pub fn asset(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|asset| asset.name == name)
    }
}

/// The newest of `releases` newer than `current`, with its version. Drafts never count, and a
/// pre-release only with `prerelease`: by GitHub's flag or by a pre-release version such as
/// `1.2.0-beta.1`, so a release marked wrongly is still not offered to everyone.
#[must_use]
pub fn newest<'a>(
    releases: &'a [Release],
    current: &Version,
    prerelease: bool,
) -> Option<(&'a Release, Version)> {
    releases
        .iter()
        .filter(|release| !release.draft)
        .filter_map(|release| release.version().map(|version| (release, version)))
        .filter(|(release, version)| prerelease || (!release.prerelease && version.pre.is_empty()))
        .filter(|(_, version)| version > current)
        .max_by(|(_, a), (_, b)| a.cmp(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str, prerelease: bool, draft: bool) -> Release {
        Release {
            tag_name: tag.to_owned(),
            draft,
            prerelease,
            html_url: format!("https://example.com/{tag}"),
            assets: Vec::new(),
        }
    }

    fn picked(releases: &[Release], current: &str, prerelease: bool) -> Option<String> {
        newest(releases, &Version::parse(current).unwrap(), prerelease)
            .map(|(release, _)| release.tag_name.clone())
    }

    #[test]
    fn the_newest_full_release_is_offered() {
        let releases = [
            release("v0.2.0", false, false),
            release("v0.3.0", false, false),
            release("v0.1.0", false, false),
            release("not-a-version", false, false),
        ];
        assert_eq!(picked(&releases, "0.1.0", false).as_deref(), Some("v0.3.0"));
        assert_eq!(picked(&releases, "0.3.0", false), None);
        assert_eq!(
            picked(&releases, "0.4.0", false),
            None,
            "never an older one"
        );
    }

    #[test]
    fn pre_releases_only_when_asked_for() {
        let releases = [
            release("v0.2.0", false, false),
            release("v0.3.0-beta.1", true, false),
            release("v0.3.0-beta.2", false, false),
            release("v0.4.0", true, false),
        ];
        assert_eq!(picked(&releases, "0.1.0", false).as_deref(), Some("v0.2.0"));
        assert_eq!(picked(&releases, "0.1.0", true).as_deref(), Some("v0.4.0"));
        assert_eq!(
            picked(&releases[..3], "0.3.0-beta.1", true).as_deref(),
            Some("v0.3.0-beta.2")
        );
    }

    #[test]
    fn drafts_are_never_offered() {
        let releases = [release("v0.2.0", false, true)];
        assert_eq!(picked(&releases, "0.1.0", true), None);
    }
}
