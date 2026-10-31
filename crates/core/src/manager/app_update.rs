//! The app's own updates (D111): the newest release on GitHub newer than this build, trusted
//! only through its manifest signed with the key the build carries, and its installer
//! downloaded and checked against that manifest before the shell may run it.

use std::path::PathBuf;
use std::sync::Arc;

use tracing::info;
use wirecatch_addons::archive;
use wirecatch_addons::manifest::Sha256Digest;
use wirecatch_engine::header::{ACCEPT, HeaderValue};
use wirecatch_engine::units::MIB;
use wirecatch_engine::{
    CancellationToken, DownloadRequest, Fetcher, Outcome, PrivateNetwork, Progress, RemoteFile, Url,
};
use wirecatch_updates::{
    Arch, FileKind, MANIFEST_FILE, Manifest, Release, SIGNATURE_FILE, System, UpdateFile, Version,
    newest,
};

use super::Manager;
use crate::error::{CoreError, Result};

/// GitHub is never on the local network.
const UPDATE_NETWORK: PrivateNetwork = PrivateNetwork::Block;
/// The most read of the list of releases, a manifest or its signature.
const MAX_METADATA: u64 = 4 * MIB;
/// The folder in the data folder installers are downloaded to.
const UPDATES_FOLDER: &str = "updates";

/// Where the app's releases are listed, and the key their manifests are signed with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppReleases {
    /// GitHub's API list of the repository's releases.
    pub list: Url,
    /// The Ed25519 public key, unpadded base64url.
    pub public_key: String,
}

/// A newer version of the app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppUpdate {
    pub version: String,
    /// The release's page.
    pub page: String,
    /// The installer the app can install itself on this computer, when there is one.
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
    /// when `kind` is given and the release has one. Pre-releases count while the setting asks
    /// for them. `None` when this build is the newest, or has nowhere to look.
    ///
    /// # Errors
    ///
    /// [`CoreError::UpdateCheckFailed`] when the releases cannot be reached or a newer one is
    /// not signed with this build's key.
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
        let listed: Vec<Release> = serde_json::from_slice(&list)
            .map_err(|e| unreachable_releases(&format!("an unreadable list ({e})")))?;
        let Some((release, version)) = newest(&listed, &current, prerelease) else {
            return Ok(None);
        };
        let manifest = self.signed_manifest(release, &releases.public_key).await?;
        if Version::parse(&manifest.version).ok().as_ref() != Some(&version) {
            return Err(refused(&format!(
                "its list of files names version {}",
                manifest.version
            )));
        }
        let download = match (kind, System::current(), Arch::current()) {
            (Some(kind), Some(system), Some(arch)) => {
                manifest.file(system, arch, kind).and_then(|file| {
                    let asset = release.asset(&file.name)?;
                    let url = Url::parse(&asset.browser_download_url).ok()?;
                    (asset.size == file.size).then(|| UpdateDownload {
                        url,
                        file: file.clone(),
                    })
                })
            }
            _ => None,
        };
        info!(%version, "a newer version of the app is out");
        Ok(Some(AppUpdate {
            version: version.to_string(),
            page: release.html_url.clone(),
            download,
        }))
    }

    /// Downloads `download` into the data folder's updates folder, reporting to `progress`, and
    /// returns its path once its size and SHA-256 match the signed manifest. A file that does not
    /// match is deleted.
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
        let target = folder.join(&file.name);
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
        let mut request = DownloadRequest::new(download.url.clone(), &folder);
        let named = target.clone();
        request.resolve_path = Some(Arc::new(move |remote: &RemoteFile| {
            if remote.total == Some(size) {
                Ok(named.clone())
            } else {
                Err(wirecatch_engine::PathRejected::new(
                    "the installer is not the size its list of files gives",
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
                    "The update could not be downloaded: {}",
                    wirecatch_messages::describe(&e, &folder, wirecatch_messages::Worked::Never)
                        .message
                )));
            }
        }
        let checked = target.clone();
        let matches = self
            .blocking(move || {
                Ok(std::fs::metadata(&checked).is_ok_and(|m| m.len() == size)
                    && archive::sha256_file(&checked).is_ok_and(|hash| hash == expected))
            })
            .await?;
        if !matches {
            let wrong = target.clone();
            self.blocking(move || {
                wirecatch_files::discard::file(&wrong);
                Ok(())
            })
            .await?;
            return Err(refused(
                "the downloaded installer does not match its list of files",
            ));
        }
        Ok(target)
    }

    /// The manifest of `release`, once its signature proves `public_key` signed it.
    async fn signed_manifest(&self, release: &Release, public_key: &str) -> Result<Manifest> {
        let asset_url = |name: &str| {
            release
                .asset(name)
                .and_then(|asset| Url::parse(&asset.browser_download_url).ok())
                .ok_or_else(|| refused(&format!("it has no {name}")))
        };
        let manifest = self
            .fetch_small(&asset_url(MANIFEST_FILE)?)
            .await
            .map_err(|e| unreachable_releases(&e))?;
        let signature = self
            .fetch_small(&asset_url(SIGNATURE_FILE)?)
            .await
            .map_err(|e| unreachable_releases(&e))?;
        let signature = String::from_utf8_lossy(&signature);
        Manifest::verified(&manifest, &signature, public_key).map_err(|e| refused(&e.to_string()))
    }

    async fn fetch_small(&self, url: &Url) -> std::result::Result<Vec<u8>, String> {
        let mut options = self.update_options();
        options.headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/vnd.github+json"),
        );
        let cancel = self.inner.stopped.child_token();
        let fetcher = Fetcher::with_private_network(url, &options, cancel, UPDATE_NETWORK)
            .await
            .map_err(|e| e.to_string())?;
        Ok(fetcher
            .get(url, None, MAX_METADATA)
            .await
            .map_err(|e| e.to_string())?
            .body
            .to_vec())
    }

    /// The app's own identity and proxy, and no request to the local network.
    fn update_options(&self) -> wirecatch_engine::DownloadOptions {
        let proxy = self.lock().app_proxy();
        crate::headers::download_options(UPDATE_NETWORK, proxy)
    }
}

fn unreachable_releases(reason: &str) -> CoreError {
    CoreError::UpdateCheckFailed(format!(
        "Wirecatch could not look for a newer version ({reason}). Check your connection, then try again."
    ))
}

fn refused(reason: &str) -> CoreError {
    CoreError::UpdateCheckFailed(format!(
        "A newer version of Wirecatch is out, but it was not offered because {reason}."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ManagerConfig;
    use base64::Engine as _;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use ed25519_dalek::{Signer as _, SigningKey};
    use sha2::{Digest as _, Sha256};
    use wirecatch_settings::folders::KnownFolders;
    use wirecatch_test_support::files::FileServer;

    const LIST: &str = "/releases";
    const INSTALLER: &[u8] = b"a newer installer";

    struct Harness {
        manager: Manager,
        server: FileServer,
        _dir: tempfile::TempDir,
    }

    fn key() -> SigningKey {
        SigningKey::from_bytes(&[7; 32])
    }

    impl Harness {
        async fn new() -> Self {
            let server = FileServer::start().await;
            let dir = tempfile::tempdir().unwrap();
            let config = ManagerConfig::for_tests(
                dir.path().join("data"),
                KnownFolders::under_home(dir.path().join("home")),
            )
            .with_app_releases(AppReleases {
                list: server.url(LIST),
                public_key: URL_SAFE_NO_PAD.encode(key().verifying_key().as_bytes()),
            });
            let manager = Manager::open(config).await.unwrap();
            Self {
                manager,
                server,
                _dir: dir,
            }
        }

        /// Publishes `tag`, its installer and its manifest signed by `signer`.
        fn publish(&self, tag: &str, prerelease: bool, signer: &SigningKey, installer: &[u8]) {
            let version = tag.trim_start_matches('v');
            let (system, arch) = (System::current().unwrap(), Arch::current().unwrap());
            let file = UpdateFile {
                name: format!("Wirecatch-{version}-setup.exe"),
                system,
                arch,
                kind: FileKind::Setup,
                size: INSTALLER.len() as u64,
                sha256: format!("{:x}", Sha256::digest(INSTALLER)),
            };
            let manifest = serde_json::to_vec(&Manifest {
                version: version.to_owned(),
                files: vec![file.clone()],
            })
            .unwrap();
            let signature = URL_SAFE_NO_PAD.encode(signer.sign(&manifest).to_bytes());
            let path = |name: &str| format!("/{tag}/{name}");
            self.server.put(&path(&file.name), installer.to_vec());
            self.server.put(&path(MANIFEST_FILE), manifest);
            self.server.put(&path(SIGNATURE_FILE), signature);
            let asset = |name: &str, size: usize| {
                serde_json::json!({
                    "name": name,
                    "browser_download_url": self.server.url(&path(name)).to_string(),
                    "size": size,
                })
            };
            let list = serde_json::json!([{
                "tag_name": tag,
                "draft": false,
                "prerelease": prerelease,
                "html_url": format!("https://example.com/{tag}"),
                "assets": [
                    asset(&file.name, INSTALLER.len()),
                    asset(MANIFEST_FILE, 1),
                    asset(SIGNATURE_FILE, 1),
                ],
            }]);
            self.server.put(LIST, serde_json::to_vec(&list).unwrap());
        }
    }

    #[tokio::test]
    async fn a_signed_newer_release_is_offered_and_its_installer_checked() {
        let h = Harness::new().await;
        h.publish("v0.2.0", false, &key(), INSTALLER);
        let update = h
            .manager
            .check_app_update("0.1.0", Some(FileKind::Setup))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(update.version, "0.2.0");
        let download = update.download.unwrap();
        let path = h
            .manager
            .fetch_app_update(
                &download,
                Arc::new(Progress::default()),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(std::fs::read(path).unwrap(), INSTALLER);
        assert_eq!(
            h.manager
                .check_app_update("0.2.0", Some(FileKind::Setup))
                .await
                .unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn a_release_signed_with_another_key_is_not_offered() {
        let h = Harness::new().await;
        h.publish(
            "v0.2.0",
            false,
            &SigningKey::from_bytes(&[8; 32]),
            INSTALLER,
        );
        let refused = h
            .manager
            .check_app_update("0.1.0", Some(FileKind::Setup))
            .await;
        assert!(
            matches!(refused, Err(CoreError::UpdateCheckFailed(_))),
            "{refused:?}"
        );
    }

    #[tokio::test]
    async fn a_pre_release_waits_for_the_setting() {
        let h = Harness::new().await;
        h.publish("v0.2.0-beta.1", true, &key(), INSTALLER);
        assert_eq!(
            h.manager.check_app_update("0.1.0", None).await.unwrap(),
            None
        );
        let mut settings = h.manager.get_settings();
        settings.prerelease_updates = true;
        h.manager.update_settings(settings).unwrap();
        let update = h
            .manager
            .check_app_update("0.1.0", None)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(update.version, "0.2.0-beta.1");
        assert_eq!(update.download, None, "no installer was asked for");
    }

    #[tokio::test]
    async fn an_installer_that_does_not_match_its_manifest_is_deleted() {
        let h = Harness::new().await;
        h.publish("v0.2.0", false, &key(), b"a different installer");
        let update = h
            .manager
            .check_app_update("0.1.0", Some(FileKind::Setup))
            .await
            .unwrap()
            .unwrap();
        let download = update.download.unwrap();
        let failed = h
            .manager
            .fetch_app_update(
                &download,
                Arc::new(Progress::default()),
                CancellationToken::new(),
            )
            .await;
        assert!(failed.is_err());
        let kept = h
            .manager
            .inner
            .data_dir
            .join(UPDATES_FOLDER)
            .join(&download.file.name);
        assert!(!kept.exists());
    }
}
