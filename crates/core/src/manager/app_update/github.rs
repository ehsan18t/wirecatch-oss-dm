use std::time::Duration;

use wirecatch_engine::Url;
use wirecatch_product::INSTALLER_PREFIX;
use wirecatch_updates::{
    Arch, Asset, FileKind, Manifest, Release, System, UpdateFile, Version, newest,
};

use super::{Manager, UpdateDownload, unreachable_releases};
use crate::error::Result;

pub(super) const RELEASES_PER_PAGE: usize = 100;
const RELEASE_SCAN_BUDGET: Duration = Duration::from_secs(60);

pub(super) async fn newest_release(
    manager: &Manager,
    list: &Url,
    current: &Version,
    prerelease: bool,
) -> Result<Option<(Release, Version)>> {
    tokio::time::timeout(
        RELEASE_SCAN_BUDGET,
        scan_releases(manager, list, current, prerelease),
    )
    .await
    .map_err(|_| unreachable_releases("GitHub's release list took too long to read"))?
}

async fn scan_releases(
    manager: &Manager,
    list: &Url,
    current: &Version,
    prerelease: bool,
) -> Result<Option<(Release, Version)>> {
    let mut latest: Option<(Release, Version)> = None;
    let mut page = 1;
    loop {
        let mut url = list.clone();
        url.query_pairs_mut()
            .append_pair("per_page", &RELEASES_PER_PAGE.to_string())
            .append_pair("page", &page.to_string());
        let listed = fetch_release_page(manager, &url).await?;
        if listed.len() > RELEASES_PER_PAGE {
            return Err(unreachable_releases(
                "GitHub returned too many releases on one page",
            ));
        }
        let newer_than = latest.as_ref().map_or(current, |(_, version)| version);
        if let Some((release, version)) = newest(
            listed.iter().filter(|release| canonical_tag(release)),
            newer_than,
            prerelease,
        ) {
            latest = Some((release.clone(), version));
        }
        if listed.len() < RELEASES_PER_PAGE {
            return Ok(latest);
        }
        page += 1;
    }
}

/// A redirect is refused: the repository's address is fixed in the build (D288).
async fn fetch_release_page(manager: &Manager, url: &Url) -> Result<Vec<Release>> {
    let list = manager
        .fetch_small(url)
        .await
        .map_err(|e| unreachable_releases(&e))?;
    if list.url != *url {
        return Err(unreachable_releases(
            "GitHub redirected the release list to another address",
        ));
    }
    serde_json::from_slice(&list.body)
        .map_err(|e| unreachable_releases(&format!("an unreadable list ({e})")))
}

fn canonical_tag(release: &Release) -> bool {
    release
        .tag_name
        .strip_prefix('v')
        .and_then(|tag| Version::parse(tag).ok())
        .is_some_and(|version| version.build.is_empty())
}

/// The normal release URL or its asset's download URL, under the configured repository.
pub(super) fn release_url(repository: &Url, tag: &str, asset: Option<&str>) -> Option<Url> {
    let mut url = repository.clone();
    {
        let mut segments = url.path_segments_mut().ok()?;
        segments.pop_if_empty().push("releases");
        if let Some(asset) = asset {
            segments.extend(["download", tag, asset]);
        } else {
            segments.extend(["tag", tag]);
        }
    }
    Some(url)
}

pub(super) fn installer_name(
    version: &Version,
    system: System,
    arch: Arch,
    kind: FileKind,
) -> Option<String> {
    wirecatch_updates::installer_name(INSTALLER_PREFIX, version, system, arch, kind)
}

pub(super) fn uploaded_asset<'a>(release: &'a Release, name: &str) -> Option<&'a Asset> {
    let mut matches = release.assets.iter().filter(|asset| asset.name == name);
    let asset = matches.next()?;
    (matches.next().is_none() && asset.is_uploaded()).then_some(asset)
}

pub(super) fn installer(
    repository: &Url,
    release: &Release,
    version: &Version,
    kind: FileKind,
) -> Option<UpdateDownload> {
    let (system, arch) = (System::current()?, Arch::current()?);
    let name = installer_name(version, system, arch, kind)?;
    let asset = uploaded_asset(release, &name)?;
    let sha256 = asset.uploaded_sha256()?.to_owned();
    let url = release_url(repository, &release.tag_name, Some(&name))?;
    Some(UpdateDownload {
        url,
        file: UpdateFile {
            name,
            system,
            arch,
            kind,
            size: asset.size,
            sha256,
        },
    })
}

pub(super) fn signed_installer(
    repository: &Url,
    release: &Release,
    version: &Version,
    manifest: &Manifest,
    kind: FileKind,
) -> Option<UpdateDownload> {
    let (system, arch) = (System::current()?, Arch::current()?);
    let file = manifest.unique_file(system, arch, kind)?;
    let name = installer_name(version, system, arch, kind)?;
    if file.name != name || uploaded_asset(release, &name)?.size != file.size {
        return None;
    }
    let url = release_url(repository, &release.tag_name, Some(&name))?;
    Some(UpdateDownload {
        url,
        file: file.clone(),
    })
}
