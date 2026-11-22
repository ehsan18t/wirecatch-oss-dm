//! The newest app release on GitHub (D111), with an installer checked against either GitHub's
//! metadata or a signed manifest, as specified by the installed build.

mod github;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use wirecatch_product::{MANIFEST_FILE, PRODUCT_NAME, SIGNATURE_FILE};

use tracing::info;
use wirecatch_addons::archive;
use wirecatch_addons::manifest::Sha256Digest;
use wirecatch_engine::header::{ACCEPT, HeaderValue};
use wirecatch_engine::units::MIB;
use wirecatch_engine::{
    CancellationToken, DownloadRequest, Fetched, Fetcher, Outcome, PrivateNetwork, Progress,
    RemoteFile, Url,
};
use wirecatch_updates::{Arch, FileKind, Manifest, Release, System, UpdateFile, Version, newest};

use super::Manager;
use crate::error::{CoreError, Result};

/// GitHub is never on the local network.
const UPDATE_NETWORK: PrivateNetwork = PrivateNetwork::Block;
/// The most read of the list of releases, a manifest or its signature.
const MAX_METADATA: u64 = 4 * MIB;
/// The folder in the data folder installers are downloaded to.
const UPDATES_FOLDER: &str = "updates";
/// The only scheme a release's page may have: it comes unsigned and goes to the system's shell.
const HTTPS: &str = "https";

/// Where the app's releases are listed and how they are verified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppReleases {
    list: Url,
    verification: ReleaseVerification,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ReleaseVerification {
    Github(Url),
    Signed {
        public_key: String,
        repository: Option<Url>,
    },
}

impl AppReleases {
    /// A custom release feed that requires an Ed25519 manifest signed by `public_key`.
    #[must_use]
    pub fn signed(list: Url, public_key: String) -> Self {
        Self {
            list,
            verification: ReleaseVerification::Signed {
                public_key,
                repository: None,
            },
        }
    }

    pub(super) fn official() -> Option<Self> {
        use wirecatch_product::{
            RELEASES_API_URL, REPOSITORY_URL, UPDATE_VERIFICATION, UpdateVerification,
        };

        let list = Url::parse(RELEASES_API_URL).ok()?;
        let repository = Url::parse(REPOSITORY_URL).ok()?;
        let verification = match UPDATE_VERIFICATION {
            UpdateVerification::Github => ReleaseVerification::Github(repository),
            UpdateVerification::Signed(public_key) => ReleaseVerification::Signed {
                public_key: public_key.to_owned(),
                repository: Some(repository),
            },
        };
        Some(Self { list, verification })
    }

    fn repository(&self) -> Option<&Url> {
        match &self.verification {
            ReleaseVerification::Github(repository) => Some(repository),
            ReleaseVerification::Signed { repository, .. } => repository.as_ref(),
        }
    }
}

/// A newer version of the app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppUpdate {
    pub version: String,
    /// The release's page, always https.
    pub page: Url,
    /// The installer to install; `None` only for a system package, whose package manager installs
    /// the update.
    pub download: Option<UpdateDownload>,
}

/// An installer to download, as the signed manifest describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateDownload {
    pub url: Url,
    pub file: UpdateFile,
}

impl Manager {
    /// The newest release newer than `current`, with the installer of `kind` for this computer
    /// when `kind` is given. Pre-releases count while the setting asks for them. `None` when this
    /// build is the newest, or has nowhere to look.
    ///
    /// # Errors
    ///
    /// [`CoreError::UpdateCheckFailed`] when the releases cannot be reached, or a newer one is
    /// not trusted by this build, has no usable installer of `kind` or no https page.
    pub async fn check_app_update(
        &self,
        current: &str,
        kind: Option<FileKind>,
    ) -> Result<Option<AppUpdate>> {
        let Some(releases) = self.inner.app_releases.clone() else {
            return Ok(None);
        };
        let current = Version::parse(current).map_err(|e| {
            CoreError::UpdateCheckFailed(format!("This build's version cannot be read ({e})."))
        })?;
        let prerelease = self.lock().settings.prerelease_updates;
        let list = self
            .fetch_small(&releases.list)
            .await
            .map_err(|e| unreachable_releases(&e))?;
        let repository = releases.repository();
        if repository.is_some() && list.url != releases.list {
            return Err(unreachable_releases(
                "GitHub redirected the release list to another address",
            ));
        }
        let listed: Vec<Release> = serde_json::from_slice(&list.body)
            .map_err(|e| unreachable_releases(&format!("an unreadable list ({e})")))?;
        let Some((release, version)) = newest(&listed, &current, prerelease) else {
            return Ok(None);
        };
        if repository.is_some() && release.tag_name != format!("v{version}") {
            return Err(refused("its tag does not match its version"));
        }
        let download = self
            .release_installer(&releases.verification, release, &version, kind)
            .await?;
        let page = match repository {
            Some(repository) => github::release_url(repository, &release.tag_name, None),
            None => Url::parse(&release.html_url)
                .ok()
                .filter(|page| page.scheme() == HTTPS),
        }
        .ok_or_else(|| refused("its page is not at a secure address"))?;
        info!(%version, "a newer version of the app is out");
        Ok(Some(AppUpdate {
            version: version.to_string(),
            page,
            download,
        }))
    }

    async fn release_installer(
        &self,
        verification: &ReleaseVerification,
        release: &Release,
        version: &Version,
        kind: Option<FileKind>,
    ) -> Result<Option<UpdateDownload>> {
        let installer = match verification {
            ReleaseVerification::Github(repository) => {
                kind.map(|kind| github::installer(repository, release, version, kind))
            }
            ReleaseVerification::Signed {
                public_key,
                repository,
            } => {
                let manifest = self
                    .signed_manifest(release, public_key, repository.as_ref())
                    .await?;
                if Version::parse(&manifest.version).ok().as_ref() != Some(version) {
                    return Err(refused(&format!(
                        "its list of files names version {}",
                        manifest.version
                    )));
                }
                kind.map(|kind| usable_installer(release, &manifest, kind, repository.as_ref()))
            }
        };
        installer
            .map(|file| {
                file.ok_or_else(|| refused("it has no verifiable installer for this computer"))
            })
            .transpose()
    }

    /// Downloads `download` into the data folder's updates folder, reporting to `progress`, and
    /// returns its path once its size and SHA-256 match the verified release metadata. A file that does not
    /// match is deleted; one already there that matches is used as it is, so trying an install
    /// again downloads nothing.
    ///
    /// # Errors
    ///
    /// [`CoreError::UpdateCheckFailed`] when it cannot be downloaded or does not match.
    pub async fn fetch_app_update(
        &self,
        download: &UpdateDownload,
        progress: Arc<Progress>,
        cancel: CancellationToken,
    ) -> Result<PathBuf> {
        let file = &download.file;
        let folder = self.inner.data_dir.join(UPDATES_FOLDER);
        let target = archive::safe_join(&folder, &file.name)
            .map_err(|_| refused("its installer has an unsafe file name"))?;
        let expected = Sha256Digest::from_hex(&file.sha256)
            .ok_or_else(|| refused("its list of files has no SHA-256"))?;
        let size = file.size;
        let made = folder.clone();
        self.blocking(move || {
            Ok(std::fs::create_dir_all(&made).map_err(|e| {
                CoreError::UpdateCheckFailed(format!(
                    "The update could not be saved in {} ({e}).",
                    made.display()
                ))
            }))
        })
        .await??;
        let checked = target.clone();
        if self
            .blocking(move || Ok(installer_matches(&checked, size, &expected)))
            .await?
        {
            return Ok(target);
        }
        let mut request = DownloadRequest::new(download.url.clone(), &folder);
        let named = target.clone();
        request.resolve_path = Some(Arc::new(move |remote: &RemoteFile| {
            if remote.total == Some(size) {
                Ok(named.clone())
            } else {
                Err(wirecatch_engine::PathRejected::new(
                    "The installer on the server is not the size this release lists. Try again later.",
                ))
            }
        }));
        request.options = self.update_options();
        match wirecatch_engine::download(request, progress, cancel).await {
            Ok(Outcome::Completed { .. }) => {}
            Ok(_) | Err(wirecatch_engine::Error::Cancelled) => {
                return Err(CoreError::UpdateCheckFailed(
                    "The update was stopped.".into(),
                ));
            }
            Err(e) => {
                return Err(CoreError::UpdateCheckFailed(format!(
                    "The update could not be downloaded. {}",
                    wirecatch_messages::describe_app_file(&e, &folder)
                )));
            }
        }
        let checked = target.clone();
        let matches = self
            .blocking(move || Ok(installer_matches(&checked, size, &expected)))
            .await?;
        if !matches {
            let wrong = target.clone();
            self.blocking(move || {
                wirecatch_files::discard::file(&wrong);
                Ok(())
            })
            .await?;
            return Err(CoreError::UpdateCheckFailed(
                "The downloaded installer does not match this release, so it was not run. Try again."
                    .into(),
            ));
        }
        Ok(target)
    }

    /// The manifest of `release`, once its signature proves `public_key` signed it.
    async fn signed_manifest(
        &self,
        release: &Release,
        public_key: &str,
        repository: Option<&Url>,
    ) -> Result<Manifest> {
        let signed_asset_url = |name: &str| {
            release
                .asset(name)
                .and_then(|asset| asset_url(repository, release, asset))
                .ok_or_else(|| refused(&format!("it has no {name}")))
        };
        let manifest = self
            .fetch_small(&signed_asset_url(MANIFEST_FILE)?)
            .await
            .map_err(|e| unreachable_releases(&e))?;
        let signature = self
            .fetch_small(&signed_asset_url(SIGNATURE_FILE)?)
            .await
            .map_err(|e| unreachable_releases(&e))?;
        let signature = String::from_utf8_lossy(&signature.body);
        Manifest::verified(&manifest.body, &signature, public_key)
            .map_err(|e| refused(&e.to_string()))
    }

    async fn fetch_small(&self, url: &Url) -> std::result::Result<Fetched, String> {
        let mut options = self.update_options();
        options.headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/vnd.github+json"),
        );
        let cancel = self.inner.stopped.child_token();
        let fetcher = Fetcher::with_private_network(url, &options, cancel, UPDATE_NETWORK)
            .await
            .map_err(|e| e.to_string())?;
        fetcher
            .get(url, None, MAX_METADATA)
            .await
            .map_err(|e| e.to_string())
    }

    /// The app's own identity and proxy, and no request to the local network.
    fn update_options(&self) -> wirecatch_engine::DownloadOptions {
        let proxy = self.lock().app_proxy();
        crate::headers::download_options(UPDATE_NETWORK, proxy)
    }
}

fn installer_matches(path: &Path, size: u64, expected: &Sha256Digest) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.len() == size)
        && archive::sha256_file(path).is_ok_and(|hash| hash == *expected)
}

/// Removes the installers of earlier runs. The setup that started this run may still hold its own
/// file, which then goes at the next start.
pub(super) fn discard_installers(data_dir: &Path) {
    wirecatch_files::discard::dir(&data_dir.join(UPDATES_FOLDER));
}

/// The installer of `kind` for this computer, when the release's file is the one the signed
/// manifest describes.
fn usable_installer(
    release: &Release,
    manifest: &Manifest,
    kind: FileKind,
    repository: Option<&Url>,
) -> Option<UpdateDownload> {
    let file = manifest.file(System::current()?, Arch::current()?, kind)?;
    let asset = release.asset(&file.name)?;
    let url = asset_url(repository, release, asset)?;
    (asset.size == file.size).then(|| UpdateDownload {
        url,
        file: file.clone(),
    })
}

fn asset_url(
    repository: Option<&Url>,
    release: &Release,
    asset: &wirecatch_updates::Asset,
) -> Option<Url> {
    match repository {
        Some(repository) => github::release_url(repository, &release.tag_name, Some(&asset.name)),
        None => Url::parse(&asset.browser_download_url).ok(),
    }
}

fn unreachable_releases(reason: &str) -> CoreError {
    CoreError::UpdateCheckFailed(format!(
        "{PRODUCT_NAME} could not look for a newer version ({reason}). Check your connection, then try again."
    ))
}

fn refused(reason: &str) -> CoreError {
    CoreError::UpdateCheckFailed(format!(
        "A newer version of {PRODUCT_NAME} is out, but it was not offered because {reason}."
    ))
}

#[cfg(test)]
mod tests;
