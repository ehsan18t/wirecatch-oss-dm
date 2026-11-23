use super::*;
use crate::ManagerConfig;
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ed25519_dalek::{Signer as _, SigningKey};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use wirecatch_settings::folders::KnownFolders;
use wirecatch_test_support::files::FileServer;

mod signed;

const LIST: &str = "/releases";
const TAG: &str = "v0.2.0";
const INSTALLER: &[u8] = b"a newer installer";

/// How a published release differs from a usual one.
#[derive(Default)]
struct Odd {
    /// What the server sends for the installer instead of the listed bytes.
    served: Option<Vec<u8>>,
    asset_size: Option<u64>,
    manifest_version: Option<&'static str>,
    signer: Option<SigningKey>,
}

struct Harness {
    manager: Manager,
    server: FileServer,
    signed: bool,
    _dir: tempfile::TempDir,
}

fn key() -> SigningKey {
    SigningKey::from_bytes(&[7; 32])
}

/// A kind this computer's build has a published installer name for.
fn kind() -> FileKind {
    if cfg!(windows) {
        FileKind::Setup
    } else {
        FileKind::AppImage
    }
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// The installer of `tag` for this computer, listed with the size and digest of `INSTALLER`.
fn installer(tag: &str) -> UpdateFile {
    let version = Version::parse(tag.trim_start_matches('v')).unwrap();
    let (system, arch) = (System::current().unwrap(), Arch::current().unwrap());
    UpdateFile {
        name: installer_name(INSTALLER_PREFIX, &version, system, arch, kind()).unwrap(),
        system,
        arch,
        kind: kind(),
        size: INSTALLER.len() as u64,
        sha256: sha256(INSTALLER),
    }
}

fn download_path(tag: &str, name: &str) -> String {
    format!("/{tag}/{name}")
}

fn release(tag: &str, prerelease: bool, assets: &[Value]) -> Value {
    json!({
        "tag_name": tag,
        "prerelease": prerelease,
        "html_url": format!("https://example.com/{tag}"),
        "assets": assets,
    })
}

impl Harness {
    async fn github() -> Self {
        Self::open(ReleaseVerification::Github).await
    }

    async fn signed() -> Self {
        let public_key = URL_SAFE_NO_PAD.encode(key().verifying_key().as_bytes());
        Self::open(ReleaseVerification::Signed { public_key }).await
    }

    async fn open(verification: ReleaseVerification) -> Self {
        let server = FileServer::start().await;
        let signed = matches!(verification, ReleaseVerification::Signed { .. });
        let dir = tempfile::tempdir().unwrap();
        let mut config = ManagerConfig::for_tests(
            dir.path().join("data"),
            KnownFolders::under_home(dir.path().join("home")),
        );
        config.app_releases = Some(AppReleases {
            list: server.url(LIST),
            verification,
        });
        let manager = Manager::open(config).await.unwrap();
        Self {
            manager,
            server,
            signed,
            _dir: dir,
        }
    }

    async fn check(&self, kind: Option<FileKind>) -> Result<Option<AppUpdate>> {
        self.manager.check_app_update("0.1.0", kind).await
    }

    async fn offered(&self, kind: Option<FileKind>) -> AppUpdate {
        self.check(kind).await.unwrap().unwrap()
    }

    async fn refusal(&self, kind: FileKind) -> String {
        let found = self.check(Some(kind)).await;
        let Err(CoreError::UpdateCheckFailed(message)) = found else {
            panic!("expected a refusal, got {found:?}");
        };
        message
    }

    async fn fetch(&self, download: &UpdateDownload) -> Result<PathBuf> {
        self.manager
            .fetch_app_update(
                download,
                Arc::new(Progress::default()),
                CancellationToken::new(),
            )
            .await
    }

    /// An asset GitHub reports as completely uploaded, served at its download address.
    fn asset(&self, tag: &str, name: &str, size: u64, sha256: &str) -> Value {
        json!({
            "name": name,
            "size": size,
            "state": "uploaded",
            "digest": format!("sha256:{sha256}"),
            "browser_download_url": self.server.url(&download_path(tag, name)).to_string(),
        })
    }

    fn list(&self, releases: &[Value]) {
        self.server.put(LIST, serde_json::to_vec(releases).unwrap());
    }

    fn publish(&self, tag: &str) -> UpdateFile {
        self.publish_odd(tag, false, &Odd::default())
    }

    /// Lists `tag` alone with its installer and, for a signed build, its signed manifest.
    fn publish_odd(&self, tag: &str, prerelease: bool, odd: &Odd) -> UpdateFile {
        let file = installer(tag);
        let served = odd.served.clone().unwrap_or_else(|| INSTALLER.to_vec());
        self.server.put(&download_path(tag, &file.name), served);
        let mut assets = vec![self.asset(
            tag,
            &file.name,
            odd.asset_size.unwrap_or(file.size),
            &file.sha256,
        )];
        if self.signed {
            let version = odd
                .manifest_version
                .unwrap_or_else(|| tag.trim_start_matches('v'));
            let signer = odd.signer.clone().unwrap_or_else(key);
            assets.extend(self.sign(tag, version, vec![file.clone()], &signer));
        }
        self.list(&[release(tag, prerelease, &assets)]);
        file
    }

    /// Uploads the manifest of `files` for `tag`, signed by `signer`, and returns its assets.
    fn sign(
        &self,
        tag: &str,
        version: &str,
        files: Vec<UpdateFile>,
        signer: &SigningKey,
    ) -> [Value; 2] {
        let manifest = serde_json::to_vec(&Manifest {
            version: version.to_owned(),
            files,
        })
        .unwrap();
        let signature = URL_SAFE_NO_PAD.encode(signer.sign(&manifest).to_bytes());
        let assets = [
            self.asset(
                tag,
                MANIFEST_FILE,
                manifest.len() as u64,
                &sha256(&manifest),
            ),
            self.asset(
                tag,
                SIGNATURE_FILE,
                signature.len() as u64,
                &sha256(signature.as_bytes()),
            ),
        ];
        self.server
            .put(&download_path(tag, MANIFEST_FILE), manifest);
        self.server
            .put(&download_path(tag, SIGNATURE_FILE), signature);
        assets
    }
}

#[tokio::test]
async fn github_mode_installs_by_the_digest_github_reports() {
    let h = Harness::github().await;
    let file = h.publish(TAG);
    let download = h.offered(Some(kind())).await.download.unwrap();
    assert_eq!(download.file, file);
    let path = h.fetch(&download).await.unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), INSTALLER);
}

#[tokio::test]
async fn github_mode_refuses_an_installer_without_a_complete_upload_and_sha256() {
    let h = Harness::github().await;
    let file = installer(TAG);
    let listed = h.asset(TAG, &file.name, file.size, &file.sha256);
    for (field, value) in [
        ("digest", Value::Null),
        ("digest", json!("sha512:abc")),
        ("digest", json!("sha256:abc")),
        ("state", json!("starter")),
        ("size", json!(0)),
    ] {
        let mut invalid = listed.clone();
        invalid[field] = value;
        h.list(&[release(TAG, false, &[invalid])]);
        let message = h.refusal(kind()).await;
        assert!(
            message.contains("no verifiable installer"),
            "{field}: {message}"
        );
    }
}

#[tokio::test]
async fn a_release_is_offered_without_an_installer_when_none_is_asked_for() {
    let h = Harness::github().await;
    h.list(&[release(TAG, false, &[])]);
    let update = h.offered(None).await;
    assert!(update.download.is_none());
    assert_eq!(update.page.as_str(), "https://example.com/v0.2.0");
}

#[tokio::test]
async fn a_release_page_that_is_not_https_is_refused() {
    let h = Harness::github().await;
    let mut listed = release(TAG, false, &[]);
    listed["html_url"] = json!("file:///C:/Windows/System32/calc.exe");
    h.list(&[listed]);
    let found = h.check(None).await;
    assert!(
        matches!(&found, Err(CoreError::UpdateCheckFailed(message)) if message.contains("page")),
        "{found:?}"
    );
}

#[tokio::test]
async fn a_pre_release_waits_for_the_setting() {
    let h = Harness::github().await;
    h.publish_odd("v0.2.0-beta.1", true, &Odd::default());
    assert_eq!(h.check(Some(kind())).await.unwrap(), None);

    let mut settings = h.manager.get_settings();
    settings.prerelease_updates = true;
    h.manager
        .change_settings(wirecatch_test_support::settings::whole_patch(&settings))
        .unwrap();
    let update = h.offered(Some(kind())).await;
    assert_eq!(update.version, "0.2.0-beta.1");
    assert_eq!(update.download.unwrap().file, installer("v0.2.0-beta.1"));
}

#[tokio::test]
async fn a_release_with_no_installer_for_this_install_is_refused() {
    let h = Harness::signed().await;
    h.publish(TAG);
    let other = if cfg!(windows) {
        FileKind::AppImage
    } else {
        FileKind::Deb
    };
    let message = h.refusal(other).await;
    assert!(message.contains("no verifiable installer"), "{message}");
}

#[tokio::test]
async fn trying_again_uses_the_checked_installer_already_downloaded() {
    let h = Harness::signed().await;
    h.publish(TAG);
    let download = h.offered(Some(kind())).await.download.unwrap();
    let first = h.fetch(&download).await.unwrap();
    let sent = h.server.requests(download.url.path());
    assert_eq!(h.fetch(&download).await.unwrap(), first);
    assert_eq!(h.server.requests(download.url.path()), sent);
}

#[tokio::test]
async fn installers_from_an_earlier_run_are_removed_when_the_app_starts() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    let updates = data.join(UPDATES_FOLDER);
    std::fs::create_dir_all(&updates).unwrap();
    std::fs::write(updates.join("Wirecatch-0.1.0-setup.exe"), INSTALLER).unwrap();
    let config = ManagerConfig::for_tests(data, KnownFolders::under_home(dir.path().join("home")));
    let _manager = Manager::open(config).await.unwrap();
    assert!(!updates.exists());
}

#[tokio::test]
async fn an_installer_of_the_wrong_size_is_refused() {
    let h = Harness::signed().await;
    let odd = Odd {
        served: Some(b"a different installer".to_vec()),
        ..Odd::default()
    };
    h.publish_odd(TAG, false, &odd);
    let download = h.offered(Some(kind())).await.download.unwrap();
    let failed = h.fetch(&download).await;
    let Err(CoreError::UpdateCheckFailed(message)) = failed else {
        panic!("expected a refusal, got {failed:?}");
    };
    assert!(message.contains("not the size"), "{message}");
}

#[tokio::test]
async fn an_installer_that_does_not_match_its_manifest_is_deleted() {
    let h = Harness::signed().await;
    let odd = Odd {
        served: Some(vec![b'x'; INSTALLER.len()]),
        ..Odd::default()
    };
    h.publish_odd(TAG, false, &odd);
    let download = h.offered(Some(kind())).await.download.unwrap();
    let failed = h.fetch(&download).await;
    let Err(CoreError::UpdateCheckFailed(message)) = failed else {
        panic!("expected a refusal, got {failed:?}");
    };
    assert_eq!(
        message,
        "The downloaded installer does not match this release, so it was not run. Try again."
    );
    let kept = h
        .manager
        .inner
        .data_dir
        .join(UPDATES_FOLDER)
        .join(&download.file.name);
    assert!(!kept.exists());
}
